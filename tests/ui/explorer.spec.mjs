// Explorer behaviour that must not regress. Fixture: tests/fixtures/gen.py
import { test, expect } from '@playwright/test';

const ID = 'https://ontosys.io/id/fixture/';
const SQLSTATE = ID + 'pg/sqlstate/22012';      // raised by 20 functions → grouped (> 12)
const CATALOG = ID + 'pg/catalog/pg_demo';      // 2 columns → not grouped
const node = (page, iri) => page.locator(`#graph g.node[data-iri="${iri}"]`);

async function open(page, prefs = {}) {
  await page.addInitScript(p => { if (!sessionStorage.getItem('init')) {
    localStorage.clear(); for (const [k, v] of Object.entries(p)) localStorage.setItem(k, v); sessionStorage.setItem('init', '1'); } }, prefs);
  await page.goto('/');
  await expect(node(page, CATALOG)).toHaveCount(1);
  await waitIdle(page);
}

// the layout has settled when no node moves for 600 ms
async function waitIdle(page) {
  const snap = () => page.evaluate(() => [...document.querySelectorAll('#graph g.node')].map(g => g.getAttribute('transform')).join(';'));
  let prev = await snap();
  for (let i = 0; i < 40; i++) {
    await page.waitForTimeout(600);
    const cur = await snap();
    if (cur === prev) return;
    prev = cur;
  }
  throw new Error('layout never settled');
}

async function positions(page) {
  return page.evaluate(() => Object.fromEntries([...document.querySelectorAll('#graph g.node')].map(g => [g.dataset.iri, g.getAttribute('transform')])));
}

async function select(page, iri) {
  await node(page, iri).locator('circle.hit').click({ force: true });
  await expect(page.locator('#graph g.node.sel')).toHaveAttribute('data-iri', iri);
}

test('real numbers from the full graph are shown', async ({ page }) => {
  await open(page);
  await expect(page.locator('#numbers')).toContainText('125');      // triples
  await expect(page.locator('#numbers')).toContainText('C functions');
  await expect(page.locator('#subtitle')).toContainText('compared with fix1');
});

test('selecting a node does not move anything else, and panel matches ring', async ({ page }) => {
  await open(page);
  const before = await positions(page);
  const linksBefore = await page.locator('#graph g.lk').count();
  await select(page, CATALOG);
  await page.waitForTimeout(800);
  const after = await positions(page);
  const moved = Object.keys(before).filter(k => k !== CATALOG && after[k] !== undefined && after[k] !== before[k]);
  expect(moved).toEqual([]);
  // the catalog's links were already drawn: selecting must not add them again (regression)
  expect(await page.locator('#graph g.lk').count()).toBe(linksBefore);
  await expect(page.locator('#detail h2')).toHaveText('pg_demo');
});

test('a large predicate becomes a group node; clicking it draws its members', async ({ page }) => {
  await open(page, { 'ontosys-view': 'focus' });
  await page.locator('#search').fill('division_by_zero');
  await page.locator('#results .row[data-iri]').first().click();
  const hub = page.locator('#graph g.node.group');
  await expect(hub).toHaveCount(1);
  await expect(hub.locator('text')).toContainText('may raise · 20');
  const before = await page.locator('#graph g.node').count();
  await hub.locator('circle.hit').click({ force: true });
  await expect(hub.locator('text')).toContainText('20 of 20');
  expect(await page.locator('#graph g.node').count()).toBe(before + 20);
});

test('drawn neighbours are never faded in "Changed only" (regression)', async ({ page }) => {
  await open(page);
  await page.locator('#deltaseg [data-v=only]').click();
  await select(page, SQLSTATE);
  const grp = page.locator('#detail .group', { hasText: 'may raise' });
  await grp.locator('button.addall').click();
  await expect(page.locator('#graph g.node.group')).toHaveCount(1);
  const opacities = await page.evaluate(() => [...document.querySelectorAll('#graph g.node.pin')].map(g => getComputedStyle(g).opacity));
  expect(opacities.length).toBeGreaterThan(20);
  expect(new Set(opacities)).toEqual(new Set(['1']));
  const selOpacity = await node(page, SQLSTATE).evaluate(g => getComputedStyle(g).opacity);
  expect(selOpacity).toBe('1');
});

test('links are coloured by family and named per the link-names toggle', async ({ page }) => {
  await open(page);
  await select(page, CATALOG);
  const colLink = page.locator(`#graph g.lk[data-s="${CATALOG}"][data-p="pg:hasColumn"]`).first();
  await expect(colLink).toHaveClass(/fam-structure/);
  await expect(colLink).toHaveClass(/\bhl\b/);
  const shown = () => page.evaluate(() => [...document.querySelectorAll('#graph g.lk text.plbl')].filter(t => getComputedStyle(t).display !== 'none').length);
  await page.locator('#plabels [data-v=sel]').click();
  const hl = await page.locator('#graph g.lk.hl').count();
  expect(await shown()).toBe(hl);
  await page.locator('#plabels [data-v=off]').click();
  expect(await shown()).toBe(0);
  await page.locator('#plabels [data-v=all]').click();
  expect(await shown()).toBe(await page.locator('#graph g.lk').count());
  await expect(page.locator('#legend .fams')).toContainText('structure');
});

test('the view choice is remembered across reloads', async ({ page }) => {
  await open(page);
  await page.locator('#viewmode [data-v=trail]').click();
  await page.reload();
  await expect(page.locator('#viewmode button.on')).toHaveAttribute('data-v', 'trail');
});

test('an entity removed since the baseline is searchable and shown from the baseline', async ({ page }) => {
  await open(page);
  await page.locator('#search').fill('f21');
  await page.locator('#results .row[data-iri]').first().click();
  await expect(page.locator('#detail .card')).toContainText('only in fix1');
  await expect(page.locator('#detail .status.removed')).toHaveCount(1);
});
