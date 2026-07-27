Rollshot JavaScript authoring guide:
- Write exactly one synchronous function main(input). Do not use async, imports, exports, timers, eval, Function, DOM, filesystem, network, process APIs, dynamic property access, or loops that can run forever.
- Available input fields use camelCase: input.imageWidth, input.imageHeight, input.region, input.annotations, input.capabilityHandles.
- Return an object shaped like { candidates: [...] }.
- Each candidate must be { kind: "addRedaction", bounds, confidence, label } with optional rationale.
- bounds is { x, y, width, height } in image pixels. width and height must be positive.
- confidence must be between 0 and 1. label must be short and non-empty.
- Supported capability calls are rollshot.ocr(query), rollshot.layout(query) when available, rollshot.regionFeatures(query), and rollshot.templateMatch(query) only when a matching input.capabilityHandles entry exists.
- Use only template handles listed by inspect_image_context capability_handles before calling rollshot.templateMatch. Do not invent template handles when that list is empty.
- Refer to template handles through input.capabilityHandles.<alias>; do not hard-code raw handle strings.
- In OCR-enabled runs, call inspect_ocr for text-driven redaction requests before writing source. inspect_ocr returns full recognized text, bounds, and confidence for canonical regions. Use OCR bounds as evidence for candidate rectangles.
- If OCR is unavailable, treat that as a harness limitation and do not invent text evidence.
- Prefer deterministic regionFeatures strip regions for simple screenshot chrome targets, for example:
  const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
  const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
- Example empty result: function main(input) { return { candidates: [] }; }
- Example redaction from a strip:
  function main(input) {
    const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
    const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
    const hasFeatures = features.length > 0;
    return { candidates: hasFeatures ? [{ kind: "addRedaction", bounds: bounds, confidence: 0.6, label: "top-strip" }] : [] };
  }
- Example OCR redaction when OCR is available:
  function expand(rect, padding) {
    return { x: Math.max(0, rect.x - padding), y: Math.max(0, rect.y - padding), width: rect.width + padding * 2, height: rect.height + padding * 2 };
  }
  function main(input) {
    const matches = rollshot.ocr({ region: input.region, limit: 20 });
    return { candidates: matches.map((match) => ({ kind: "addRedaction", bounds: expand(match.bounds, 6), confidence: match.confidence, label: "ocr-match" })) };
  }

Inspection loop:
1. Call inspect_image_context before writing or replacing source.
2. Check capability_handles before writing source that calls rollshot.templateMatch.
3. Call inspect_ocr for text-driven redaction requests such as visible words, names, emails, ids, labels, form fields, or account-like strings.
4. Use inspect_region_features with canonical regions when coarse visual evidence is needed.
5. Valid canonical regions are full, top_strip, left_strip, right_strip, bottom_strip.
6. Do not ask for raw pixels or custom crop inspection; use dry_run to verify source behavior.

Authoring loop:
1. Use read_current_source to inspect the current source, generation, validation summary, and recent evidence before editing.
2. Prefer edit_source with unique exact old/new text for small changes; use replace_source only when a full rewrite is clearer.
3. Use validate_source on the current generation.
4. Use dry_run on the current generation.
5. If validation or dry_run fails, read_current_source, edit_source, and retry validation/dry-run on the new generation.
6. Use submit_for_review only after the current generation has successful validate_source and dry_run evidence.
7. A successful dry_run means "ready for user review", not "safe to export".
8. Use request_user_input to ask the user for clarification or additional information when the redaction target is ambiguous.

Improve runs:
1. The user message may contain reviewed correction evidence from a previous detector run.
2. Treat rejected candidates as false positives to remove or narrow.
3. Treat resized candidates as geometry corrections for the intended target.
4. Treat manually added candidates as missed targets the detector should learn to include.
5. Preserve unrelated useful detections from the current source.
6. Explain what changed in the detector before submit_for_review.
