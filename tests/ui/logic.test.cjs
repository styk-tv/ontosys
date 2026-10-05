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

test('localName: last segment after # or /, for graphs in any namespace', () => {
  assert.equal(L.localName('http://codeontology.org/ontology/Function'), 'Function');
  assert.equal(L.localName('http://usefulinc.com/ns/doap#Project'), 'Project');
  assert.equal(L.localName('http://example.org/data/project/demo/import/os.path'), 'os.path');
  assert.equal(L.localName('http://example.org/a/b/'), 'b');
  assert.equal(L.localName('urn:x'), 'urn:x');
  assert.equal(L.localName(''), '');
});

test('shortName: known prefixes are stripped, anything else is its local name', () => {
  assert.equal(L.shortName('cx:Function'), 'Function');
  assert.equal(L.shortName('pg:SQLState'), 'SQLState');
  assert.equal(L.shortName('http://example.org/code/Struct'), 'Struct');
});

test('displayName: the label when there is one, else the local name of the IRI', () => {
  assert.equal(L.displayName('handler_00', 'http://example.org/x/fn/handler_00'), 'handler_00');
  assert.equal(L.displayName(null, 'http://example.org/data/project/demo/import/re'), 're');
  assert.equal(L.displayName('', 'http://example.org/x/y'), 'y');
});

test('hashColor: stable per class and spread across the palette', () => {
  const a = 'http://codeontology.org/ontology/Function';
  assert.equal(L.hashColor(a), L.hashColor(a));
  assert.match(L.hashColor(a), /^#[0-9a-f]{6}$/);
  const many = ['Function', 'Parameter', 'Import', 'Struct', 'Constant', 'Trait', 'Module', 'Enum']
    .map(c => L.hashColor('http://example.org/code/' + c));
  assert.ok(new Set(many).size >= 5, 'different classes mostly get different colours');
});

test('classId: class nodes are group nodes of their own kind', () => {
  const id = L.classId('http://codeontology.org/ontology/Function');
  assert.ok(L.isClassNode(id));
  assert.ok(L.isHub(id), 'class nodes behave as group nodes on the canvas');
  assert.ok(!L.isClassNode(L.hubId('x', 'in', 'pg:mayRaise')));
  assert.equal(L.classOf(id), 'http://codeontology.org/ontology/Function');
});
