# Smart Redaction Agent Phase B2 OCR Inspection

**Date:** 2026-06-27
**Status:** Complete

## Goal

Give the Smart Redaction authoring agent a truthful OCR inspection tool that can
return full recognized text, coordinates, and confidence for the current
screenshot when OCR is compiled and prepared.

Phase B1 gave the model bounded image dimensions and region-feature evidence.
Phase B2 adds text evidence for text-driven redaction requests while preserving
the same prepared-capability contract: observations exposed to the model must
come from the same prepared host state used by dry-run JavaScript.

## Product and Privacy Assumption

Phase B2 may return full OCR text to the model. The workbench already presents
provider/context risk before the user starts a Smart Redaction authoring run, so
this phase does not redact, hash, classify, or truncate recognized text in
`inspect_ocr` results.

This phase does not change the disclosure modal or payload modes. The existing
`PayloadMode::FullScreenshot` behavior remains the product path for OCR-enabled
inspection. The existing `PayloadMode::OcrLayoutOnly` option remains out of
scope and must not be reinterpreted as a real local-summary-only provider mode
in this phase.

## Success Criteria

- OCR-enabled product Smart Redaction builds expose a real `inspect_ocr` tool.
- Default product builds that do not compile OCR do not expose a fake working
  `inspect_ocr` tool.
- `inspect_image_context` reports OCR availability truthfully for default and
  OCR-enabled builds.
- `inspect_ocr` accepts only named canonical regions, not arbitrary crop
  coordinates.
- OCR tool results include full recognized text, OCR bounds, confidence, and
  stable unavailable/error codes.
- OCR inspection results stay within the existing tool result-byte budget.
- JavaScript dry-run OCR calls and `inspect_ocr` use the same prepared host
  cache for the same canonical query.
- The Smart Redaction authoring guide instructs the model to call `inspect_ocr`
  for text-driven redaction requests.
- Region-feature inspection from B1 remains unchanged.

## Scope

### In Scope

- Add an app-level Cargo feature that forwards OCR support to
  `rollshot-vision/ocr`.
- Add a product-owned canonical OCR inspection catalog.
- Prepare bounded OCR queries before OCR-enabled agent runs.
- Replace or extend the agent OCR inspection stub with a real prepared-host
  tool.
- Register `inspect_ocr` in the Smart Redaction product registry only when OCR
  is compiled and prepared.
- Return full OCR text in tool results.
- Update prompt guidance and provider contract tests so text-driven intents use
  OCR inspection.
- Add tests for default-build unavailability and OCR-enabled availability.

### Out of Scope

- OCR text masking, hashing, classification, or truncation.
- `PayloadMode::OcrLayoutOnly` data-flow changes.
- Layout inspection.
- Template-handle visibility or template-match inspection.
- Source read/patch tools.
- Arbitrary OCR crop inspection.
- OCR model packaging changes in `rollshot-ocr`.
- Workbench UI redesign.

## Design

### 1. OCR Feature Gate

`rollshot-app` currently depends on `rollshot-vision` without enabling OCR.
B2 should add an app feature:

```toml
[features]
ocr = ["rollshot-vision/ocr"]
```

The default workspace build remains OCR-disabled. OCR-enabled product tests run
with `-p rollshot-app --features ocr` and may build `rollshot-ocr`.

All product registry behavior must be gated by compile-time OCR availability.
Default builds should continue to report OCR as unavailable and should not
register a fake `inspect_ocr` tool in Smart Redaction runs.

### 2. Canonical OCR Catalog

B2 should mirror B1's region-feature catalog pattern, but for OCR. The catalog
is workbench-owned product authoring context, not a new automation language
concept.

Each OCR catalog entry contains:

- canonical name,
- bounds in image coordinates,
- matching `OcrQuery` when the region can be prepared,
- skipped/unavailable reason when it cannot be prepared.

The first OCR catalog should use the same canonical names exposed in B1:

- `full`
- `top_strip`
- `left_strip`
- `right_strip`
- `bottom_strip`

`full` is preferred for text-driven screenshots because users often ask for
emails, names, account ids, form fields, or labels anywhere in the image. Strip
regions remain useful as bounded fallbacks and preserve a consistent inspection
vocabulary.

The catalog must respect `rollshot-vision` OCR area limits. Oversized regions
stay visible in `inspect_image_context` as unavailable entries with a stable
reason such as `area_limit_exceeded`.

### 3. OCR Preparation

When OCR is compiled, the workbench should prepare OCR queries before entering
the agent loop:

1. Build `VisualIndex`.
2. Prepare B1 region features exactly as today.
3. Build the OCR catalog.
4. Call `RealAutomationHost::prepare_ocr` for each OCR catalog entry with a
   query.
5. Store the same OCR catalog entries in `AuthoringInspectionContext` as a
   separate OCR inspection list.

Do not overload the B1 region-feature entries for OCR. Region-feature
inspection entries carry `RegionFeaturesQuery`; OCR inspection entries must
carry `OcrQuery`. Keeping the lists separate prevents an OCR tool from
accidentally reusing region-feature preparation metadata.

Preparation failures for individual OCR regions should not fabricate results.
They should become structured unavailable catalog entries when the failure is a
bounded capability condition, such as area limits. Unexpected OCR initialization
or detection failures may fail vision preparation for the run, because the
product would otherwise advertise an OCR-enabled tool backed by no reliable OCR
state.

When OCR is not compiled, the workbench should not call OCR preparation. It
should report OCR unavailable with a stable reason such as `ocr_disabled`.

### 4. `inspect_image_context`

Extend the B1 image context result so OCR availability is based on the OCR
catalog:

- `available` when at least one canonical OCR region has been prepared,
- `partial` when at least one canonical OCR region is prepared and at least one
  is skipped,
- `unavailable` when no OCR region is prepared.

Default OCR-disabled builds report `unavailable` with `ocr_disabled`.

The result should include OCR canonical region availability separately from the
B1 `regions` list, using a field such as `ocr_regions`. Each OCR region entry
should include name, bounds, status, and unavailable reason when applicable. It
must not include OCR text; text appears only in `inspect_ocr`.

The image context should continue to include region-feature availability exactly
as B1 defined it. Layout and template-match availability remain unchanged.

### 5. `inspect_ocr`

Replace the product OCR stub with a real bounded inspection tool backed by the
prepared `AutomationHost`.

Arguments:

- `region`: one canonical region name: `full`, `top_strip`, `left_strip`,
  `right_strip`, or `bottom_strip`.

Result shape:

- `region`: canonical name.
- `status`: `available` or `unavailable`.
- `bounds`: canonical bounds when known.
- `matches`: bounded list of OCR matches.
- `unavailable_reason`: structured code when unavailable.

Each match includes:

- `bounds`,
- `text`,
- `confidence`.

The tool must not accept arbitrary rectangles. It should call
`AutomationHost::ocr` with the catalog query and convert capability errors into
structured unavailable responses. It should not synthesize OCR matches if the
host cache is missing.

`inspect_ocr` should use a small fixed query limit that fits the existing tool
result-byte budget. If OCR returns too many matches or too much text for the
registry result limit, the registry's result-byte budget error remains a hard
tool error rather than silently dropping data.

### 6. Registry and Prompt Contract

OCR-enabled Smart Redaction product registries should expose:

- `replace_source`
- `validate_source`
- `submit_for_review`
- `request_user_input`
- `inspect_context_summary`
- `inspect_image_context`
- `inspect_region_features`
- `inspect_ocr`
- `dry_run`

Default OCR-disabled product registries should continue to exclude
`inspect_ocr`.

The Smart Redaction authoring guide should instruct the model to:

- call `inspect_image_context` before writing or replacing source,
- call `inspect_ocr` when the user asks about text, visible words, names,
  emails, ids, labels, form fields, or account-like strings,
- use OCR match bounds as evidence for candidate rectangles,
- validate and dry-run before submit,
- treat OCR unavailable responses as harness limitations, not a reason to invent
  text evidence.

The guide may keep OCR JavaScript examples, but it should distinguish
pre-authoring OCR inspection from dry-run JavaScript OCR calls.

### 7. Error Handling

- Unknown canonical region names are recoverable argument errors.
- Skipped catalog entries return `status: "unavailable"` with a stable reason
  such as `area_limit_exceeded`.
- OCR-disabled builds report OCR unavailable in `inspect_image_context` and do
  not register `inspect_ocr` in product runs.
- Host capability errors return `status: "unavailable"` with the host code, such
  as `vision_index_unavailable`, `capability_unavailable`, or
  `limit_exceeded`.
- Tool result-byte budget failures remain hard `ToolRegistry` errors.
- Empty or invalid screenshots continue to fail during vision context
  preparation before the agent run starts.

## Testing

### Agent Tool Tests

- `inspect_ocr` schema is an object schema.
- `inspect_ocr` requires a canonical region enum.
- `inspect_ocr` rejects unknown canonical names as a recoverable argument
  error.
- `inspect_ocr` returns full text, bounds, and confidence from a prepared fake
  host.
- `inspect_ocr` returns unavailable for skipped regions.
- `inspect_ocr` converts host errors into structured unavailable results.
- `inspect_image_context` reports OCR availability from the OCR inspection
  context.

### Workbench Tests

- Default product registry excludes `inspect_ocr`.
- OCR-enabled product registry includes `inspect_ocr`.
- Default image context reports OCR unavailable with `ocr_disabled`.
- OCR-enabled image context reports OCR available or partial based on prepared
  catalog entries.
- OCR preparation prepares only catalog entries with queries.
- Oversized OCR regions stay in the catalog with unavailable reasons.
- A synthetic OCR-capable image can prepare context, inspect `full`, and dry-run
  an OCR preset using the same canonical query.

### Prompt and Provider Tests

- The system prompt tells the model to use `inspect_ocr` for text-driven
  redaction intents.
- OCR-enabled tool definitions include `inspect_ocr`.
- OCR-disabled product registry tests continue to prove no fake OCR tool is
  exposed.

## Verification Commands

Use narrow default-build checks first:

```bash
rtk cargo test -p rollshot-agent inspect_ocr
rtk cargo test -p rollshot-agent inspect_image_context
rtk cargo test -p rollshot-app result_workspace::workbench
```

Then run OCR-enabled workbench and vision checks:

```bash
rtk cargo test -p rollshot-app --features ocr result_workspace::workbench
rtk cargo test -p rollshot-vision --features ocr ocr
```

Before claiming Phase B2 complete:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-app --features ocr result_workspace::workbench
rtk cargo fmt --check
```

Run wider workspace tests if the implementation changes shared automation,
vision, or provider contracts.

## Risks

- Full OCR text can contain sensitive user data. B2 accepts this risk because
  Smart Redaction already asks for provider-context consent before authoring
  runs. Do not reuse B2 tool results in a no-upload or local-summary-only mode
  without a separate payload-mode design.
- OCR model initialization can be expensive or fail due to runtime dependencies.
  OCR-enabled registration must reflect actual prepared availability, not just a
  compiled feature flag.
- If `inspect_ocr` and dry-run prepare different queries, the model may see OCR
  evidence that JavaScript cannot reproduce. The OCR catalog must be shared by
  preparation, inspection, and dry-run expectations.
- OCR text can be long. Tool result budget failures should remain visible rather
  than silently truncating in this phase.
- Default builds must remain fast and should not pull in OCR dependencies unless
  explicitly built with OCR enabled.

## Deferred Work

- `PayloadMode::OcrLayoutOnly` as a real local OCR summary provider mode.
- OCR text redaction, hashing, classification, or configurable truncation.
- Layout inspection.
- Template-match handle visibility and lifecycle.
- Source read/patch editing ergonomics.
- Fixture-level model evaluation for whether OCR inspection improves preset
  authoring quality.
