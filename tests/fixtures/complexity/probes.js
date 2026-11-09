function transform(x) { return x.items.sort(); }
function mapNamed(xs) { return xs.map(transform); }
function mapParam(xs, f) { return xs.map(f); }
class P { includes(x) { return false; } }
class Q { includes(x) { return false; } }
function overlap(xs, ys) { let c = 0; for (const y of ys) { if (xs.includes(y)) c++; } return c; }
function countDown(a, n) { let s = 0; for (let i = n - 1; i >= 0; i--) { s += a[i]; } return s; }
function bfs(root) { const queue = [root]; for (let i = 0; i < queue.length; i++) { const n = queue[i]; for (const c of n.children) { queue.push(c); } } return queue; }
function twiceJs(a, n) { let s = 0; for (let i = 0; i < 2 * n; i++) s += a[i]; return s; }
function sortCmp(xs) { return xs.sort((a, b) => a.name.localeCompare(b.name)); }
function skipJs(xs, n) { for (const x of xs) { if (x) continue; for (let i = 0; i < n; i++) {} } }
function* genJs(xs) { for (const x of xs) yield transform(x); }
function useGen(xs) { return genJs(xs).next(); }
function poke(o) { return o.frobnicate(); }
function walkList(node) { let c = 0; while (node) { c++; node = node.next; } return c; }
