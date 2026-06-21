function main(input) {
  const method = "ocr";
  const matches = rollshot[method]({ region: input.region, limit: 10 });
  return { candidates: matches };
}
