function first(value) {
  return second(value);
}

function second(value) {
  return first(value);
}

function main(input) {
  return { candidates: first(input.region) };
}
