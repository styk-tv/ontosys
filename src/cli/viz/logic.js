// OntoSys explorer — pure decision logic (no DOM), shared by the page and the
// unit tests (`node --test tests/ui`). Everything the canvas decides about
// identity, grouping, colour family and visibility lives here.
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.OntoLogic = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  /** More targets than this under one predicate are drawn as a group node. */
  const GROUP_MAX = 12;
  /** Members fetched per click on a group node. */
  const GROUP_PAGE = 24;

  const PREFIXES = [
    ['https://ontosys.io/ns/c#', 'cx:'],
    ['https://ontosys.io/ns/pg#', 'pg:'],
    ['https://ontosys.io/ns/pgcat#', 'pgcat:'],
    ['http://www.w3.org/2000/01/rdf-schema#', 'rdfs:'],
    ['http://www.w3.org/1999/02/22-rdf-syntax-ns#', 'rdf:'],
    ['http://www.w3.org/2002/07/owl#', 'owl:'],
  ];

  /** One spelling per predicate: full IRIs and prefixed names map to the prefixed name. */
  function pk(p) {
    if (!p) return '';
    for (const [ns, short] of PREFIXES) if (p.startsWith(ns)) return short + p.slice(ns.length);
    return p;
  }

  const FAMILIES = {
    structure: ['pg:partOf', 'pg:inArea', 'cx:definedIn', 'cx:declaredIn', 'pg:hasColumn', 'pg:hasIndex',
      'cx:hasField', 'cx:hasMember', 'pg:inCatalog', 'pg:inClass', 'cx:includes', 'pg:representedBy'],
    reference: ['cx:calls', 'pg:lookupCatalog', 'cx:declares', 'cx:usesType', 'cx:fieldType',
      'pg:specializes', 'pg:argumentType'],
    behaviour: ['pg:mayRaise', 'pg:readsSetting', 'pg:createsNode', 'pg:inspectsNode', 'pg:operatesOn',
      'pg:waitsOn', 'pg:usesLock', 'pg:usesLockTranche', 'pg:touchesHook', 'pg:boundToVariable',
      'pg:checkHook', 'pg:assignHook', 'pg:showHook'],
    sql: ['pg:implementsSQLFunction', 'pg:implementedBy'],
    type: ['rdf:type', 'rdfs:subClassOf'],
  };
  const FAMILY_OF = new Map();
  for (const [fam, ps] of Object.entries(FAMILIES)) for (const p of ps) FAMILY_OF.set(p, fam);

  /** Colour family of a predicate: structure | reference | behaviour | sql | type | other. */
  function family(p) {
    const k = pk(p);
    if (FAMILY_OF.has(k)) return FAMILY_OF.get(k);
    if (k.startsWith('pgcat:')) return 'reference';   // catalog rows pointing at catalog rows
    return 'other';
  }

  /** Draw a predicate's targets as one group node instead of individual spokes? */
  function shouldGroup(total) {
    return Number(total) > GROUP_MAX;
  }

  /** Stable id of the group node for (owner, direction, predicate). */
  function hubId(owner, dir, p) {
    return 'group:' + (dir === 'in' ? 'in' : 'out') + ':' + pk(p) + ':' + owner;
  }

  function isHub(id) {
    return typeof id === 'string' && id.startsWith('group:');
  }

  /** Identity of a drawn link; the same relation always yields the same key. */
  function linkKey(s, p, t) {
    return s + '|' + pk(p) + '|' + t;
  }

  /**
   * Which node ids are shown.
   *  all   → null (everything drawn so far)
   *  focus → the selection plus its neighbourhood (direct neighbours, group nodes, loaded members)
   *  trail → focus, plus every selection walked through up to the current history position
   */
  function visibleIds(mode, sel, focus, hist, hpos) {
    if (mode === 'all' || !sel) return null;
    const keep = new Set(focus || []);
    keep.add(sel);
    if (mode === 'trail') (hist || []).slice(0, (hpos ?? -1) + 1).forEach(i => keep.add(i));
    return keep;
  }

  return { GROUP_MAX, GROUP_PAGE, pk, family, FAMILIES, shouldGroup, hubId, isHub, linkKey, visibleIds };
});
