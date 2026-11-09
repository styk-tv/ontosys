pub fn check(v: &Vec<i32>, x: i32) { assert!(v.contains(&x)); }
pub fn sum_down(n: usize) -> usize { let mut i = n; let mut s = 0; while i > 0 { s += i; i -= 1; } s }
pub fn sort_names(v: &mut Vec<String>) { v.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase())); }
pub fn heavy_rs(x: &Vec<i32>) -> usize { let mut c = 0; for _ in x.iter() { c += 1; } c }
pub fn map_named_rs(v: &Vec<Vec<i32>>) -> Vec<usize> { v.iter().map(heavy_rs).collect() }
pub fn skip_cont(v: &[i32], n: usize) { for x in v { if *x > 0 { continue; } for _ in 0..n {} } }
pub fn twice_rs(n: usize) -> usize { let mut t = 0; for i in 0..2 * n { t += i; } t }
pub fn from_k(v: &mut [i32], k: usize, n: usize) { for i in k..n { v[i] += 1; } }
pub fn down_range(n: usize) -> usize { let mut t = 0; for i in (0..n).rev() { t += i; } t }
pub fn drain_stack(mut stack: Vec<i32>) -> i32 { let mut s = 0; while let Some(x) = stack.pop() { s += x; } s }
pub fn dfs(mut stack: Vec<Vec<i32>>) { while let Some(x) = stack.pop() { if x.len() > 1 { stack.push(x[1..].to_vec()); } } }
pub fn fmt_len(v: &Vec<i32>) -> String { format!("{}", heavy_rs(v)) }
