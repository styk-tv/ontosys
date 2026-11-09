function total(xs) { let s = 0; for (const x of xs) { s += x; } return s; }
function bsearch(xs, x) { let lo = 0, hi = xs.length - 1; while (lo <= hi) { const mid = Math.floor((lo + hi) / 2); if (xs[mid] === x) return mid; if (xs[mid] < x) lo = mid + 1; else hi = mid - 1; } return -1; }
function pairs(xs) { let c = 0; for (const x of xs) { for (const y of xs) { if (x === y) c++; } } return c; }
function ten() { let s = 0; for (let i = 0; i < 10; i++) { s += i; } return s; }
function first(xs) { let i = 0; while (true) { if (xs[i]) break; i++; } return i; }
function dup(xs, ys) { let c = 0; for (const x of xs) { if (ys.includes(x)) c++; } return c; }
function sorts(rows) { for (const row of rows) { row.sort(); } }
function find(xs, x) { for (let i = 0; i < xs.length; i++) { if (xs[i] === x) return i; } return -1; }
function cnt(s) { let c = 0; for (let i = 0; i < s.indexOf('x'); i++) { c++; } return c; }
function fib(n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); }
function grow(n) { for (let i = 0; i < n; i++) { if (i === 3) n++; } }
function u(n) { return frobnicate(n); }
function e() {}
