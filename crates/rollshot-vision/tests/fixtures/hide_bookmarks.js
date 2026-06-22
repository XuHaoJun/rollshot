function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.bookmarkStrip,
    region: { kind: "full" },
    limit: 40,
  });
  return {
    candidates: matches
      .filter((match) => match.score >= 0.82)
      .map((match) => ({
        kind: "addRedaction",
        bounds: match.bounds,
        confidence: Math.min(0.95, match.score),
        label: "bookmark-strip-template",
      })),
  };
}
