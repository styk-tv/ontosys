// Unit tests for the explorer's decision logic: `node --test tests/ui`
const test = require('node:test');
const assert = require('node:assert/strict');
const L = require('../../src/cli/viz/logic.js');

test('pk: full IRI and prefixed name are the same predicate', () => {
  assert.equal(L.pk('https://ontosys.io/ns/pg#hasColumn'), 'pg:hasColumn');
  assert.equal(L.pk('pg:hasColumn'), 'pg:hasColumn');
  assert.equal(L.pk('http://www.w3.org/1999/02/22-rdf-syntax-ns#type'), 'rdf:type');
  assert.equal(L.pk('http://example.org/other'), 'http://example.org/other');
});

test('linkKey: the overview spelling and the panel spelling collide (no duplicate links)', () => {
  // regression: the overview sent pg:hasColumn, the panel sent the full IRI,
  // and every click added the selection's links a second time.
  assert.equal(L.linkKey('a', 'pg:hasColumn', 'b'), L.linkKey('a', 'https://ontosys.io/ns/pg#hasColumn', 'b'));
});

test('family: predicates fall into the agreed colour families', () => {
  assert.equal(L.family('pg:partOf'), 'structure');
  assert.equal(L.family('https://ontosys.io/ns/c#definedIn'), 'structure');
  assert.equal(L.family('cx:calls'), 'reference');
  assert.equal(L.family('pgcat:prorettype'), 'reference');
  assert.equal(L.family('pg:mayRaise'), 'behaviour');
  assert.equal(L.family('pg:readsSetting'), 'behaviour');
  assert.equal(L.family('pg:implementsSQLFunction'), 'sql');
  assert.equal(L.family('rdf:type'), 'type');
  assert.equal(L.family('cx:bodyHash'), 'other');
});

test('shouldGroup: more than GROUP_MAX targets become a group node', () => {
  assert.equal(L.GROUP_MAX, 12);
  assert.equal(L.shouldGroup(12), false);
  assert.equal(L.shouldGroup(13), true);
  assert.equal(L.shouldGroup(26813), true);
  assert.equal(L.shouldGroup('7'), false);
});

test('hubId: stable per owner, direction and predicate; recognisable', () => {
  const a = L.hubId('x', 'in', 'https://ontosys.io/ns/pg#mayRaise');
  assert.equal(a, L.hubId('x', 'in', 'pg:mayRaise'));
  assert.notEqual(a, L.hubId('x', 'out', 'pg:mayRaise'));
  assert.notEqual(a, L.hubId('y', 'in', 'pg:mayRaise'));
  assert.ok(L.isHub(a));
  assert.ok(!L.isHub('https://ontosys.io/id/fixture/fn/f01'));
});

test('visibleIds: all shows everything, focus the neighbourhood, trail adds the walked path', () => {
  const focus = new Set(['n1', 'n2', 'group:in:pg:mayRaise:s']);
  assert.equal(L.visibleIds('all', 's', focus, ['a', 's'], 1), null);
  assert.deepEqual([...L.visibleIds('focus', 's', focus, ['a', 'b', 's'], 2)].sort(),
    ['group:in:pg:mayRaise:s', 'n1', 'n2', 's']);
  const trail = L.visibleIds('trail', 's', focus, ['a', 'b', 's', 'future'], 2);
  assert.ok(trail.has('a') && trail.has('b') && trail.has('s'));
  assert.ok(!trail.has('future'), 'forward history is not part of the trail');
  assert.equal(L.visibleIds('focus', null, focus, [], -1), null, 'nothing selected: show everything');
});
