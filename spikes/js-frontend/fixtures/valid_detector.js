const matches = rollshot.ocr({ region: "full" })
  .filter((m) => m.confidence > 0.8)
  .map((m) => ({ x: m.x, y: m.y, w: m.w, h: m.h }));
return { candidates: matches };
