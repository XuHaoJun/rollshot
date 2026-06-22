function main(input) {
  const strip = {
    kind: "rect",
    bounds: { x: 0, y: 0, width: input.imageWidth, height: 12 },
  };
  const features = rollshot.regionFeatures({ region: strip, limit: 1 });
  return {
    candidates: features
      .filter((f) => f.edgeDensity < 0.15)
      .map((f) => ({
        kind: "addRedaction",
        bounds: f.bounds,
        confidence: 0.7,
        label: "top-bar-region",
      })),
  };
}
