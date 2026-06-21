function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}

function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 10 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: expandBounds(match.bounds, 8),
      confidence: match.confidence,
      label: "ocr-match",
    })),
  };
}
