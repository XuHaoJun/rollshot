function f(n){ return n <= 0 ? [] : f(n - 1); } return { candidates: f(3) };
