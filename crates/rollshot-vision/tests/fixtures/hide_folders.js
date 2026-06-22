function padToCaption(bounds) {
  return {
    x: Math.max(0, bounds.x - 8),
    y: Math.max(0, bounds.y - 8),
    width: bounds.width + 16,
    height: bounds.height + 36,
  };
}

function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.folderIcon,
    region: { kind: "full" },
    limit: 80,
  });
  return {
    candidates: matches
      .filter((match) => match.score >= 0.8)
      .map((match) => ({
        kind: "addRedaction",
        bounds: padToCaption(match.bounds),
        confidence: Math.min(0.94, match.score),
        label: "desktop-folder-icon",
      })),
  };
}
