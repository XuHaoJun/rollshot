function inspect(region) {
  return rollshot.ocr({ region, limit: 10 });
}

function main(input) {
  return { candidates: inspect(input.region) };
}
