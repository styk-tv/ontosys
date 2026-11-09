fn total(xs: &[i64]) -> i64 { let mut s = 0; for x in xs { s += x; } s }
fn bsearch(xs: &[i64], x: i64) -> i64 { let (mut lo, mut hi) = (0i64, xs.len() as i64 - 1); while lo <= hi { let mid = (lo + hi) / 2; if xs[mid as usize] == x { return mid; } if xs[mid as usize] < x { lo = mid + 1; } else { hi = mid - 1; } } -1 }
fn pairs(xs: &[i64]) -> usize { let mut c = 0; for x in xs { for y in xs { if x == y { c += 1; } } } c }
fn ten() -> i64 { let mut s = 0; for i in 0..10 { s += i; } s }
fn first(xs: &[bool]) -> usize { let mut i = 0; loop { if xs[i] { break; } i += 1; } i }
fn dup(xs: &[i64], ys: &Vec<i64>) -> usize { let mut c = 0; for x in xs { if ys.contains(x) { c += 1; } } c }
fn sorts(rows: &mut Vec<Vec<i64>>) { for row in rows.iter_mut() { row.sort(); } }
fn find(xs: &[i64], x: i64) -> Option<usize> { for i in 0..xs.len() { if xs[i] == x { return Some(i); } } None }
fn fib(n: u64) -> u64 { if n < 2 { return n; } fib(n - 1) + fib(n - 2) }
fn grow(mut n: usize) { let mut i = 0; while i < n { if i == 3 { n += 1; } i += 1; } }
fn u(n: u64) -> u64 { frobnicate(n) }
fn e() {}
