function main(input) {
  const value = Reflect.get(input, "region");
  return { candidates: value };
}
