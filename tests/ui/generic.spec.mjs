// The explorer on a graph with no PostgreSQL grounding, in namespaces ontosys
// does not know (the shape of today's Rust / Python / TypeScript output).
// Fixture: tests/fixtures/gen_generic.py, served on :3902 with --overview-above 0.
import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:3902';
const CO = 'http://codeontology.org/ontology/';
const DEMO = 'http://example.org/data/project/demo/';
const FUNCTION = 'class:' + CO + 'Function';
const node = (page, iri) => page.locator(`#graph g.node[data-iri="${iri}"]`);
const members = page => page.locator(`#graph g.node[data-iri^="${DEMO}fn/handler_"]`);

async function open(page) {
  await page.addInitScript(() => { if (!sessionStorage.getItem('init')) { localStorage.clear(); sessionStorage.setItem('init', '1'); } });
  await page.goto(BASE + '/');
  await expect(node(page, FUNCTION)).toHaveCount(1);
  await waitIdle(page);
}

// the layout has settled when no node moves for 600 ms
async function waitIdle(page) {
  const snap = () => page.evaluate(() => [...document.querySelectorAll('#graph g.node')].map(g => g.getAttribute('transform')).join(';')
    + document.querySelector('#graph > g').getAttribute('transform'));
  let prev = await snap();
  for (let i = 0; i < 40; i++) {
    await page.waitForTimeout(600);
    const cur = await snap();
    if (cur === prev) return;
    prev = cur;
  }
  throw new Error('layout never settled');
}

test('an ungrounded graph opens on a map of its classes', async ({ page }) => {
  await open(page);
  await expect(page.locator('#graph g.node.cls')).toHaveCount(5);   // Function, Parameter, Import, Struct, Project
  await expect(node(page, FUNCTION).locator('text')).toHaveText('Function · 30');
  const hasMethod = page.locator(`#graph g.lk[data-t="${FUNCTION}"]`);
  await expect(hasMethod).toHaveCount(1);
  await expect(hasMethod.locator('text')).toHaveText('hasMethod · 30');
  await expect(page.locator('#mode')).toContainText('classes');
  await expect(page.locator('#mode')).toContainText('2 invalid lines skipped');
});

test('clicking a class draws a page of its members, clicking again the rest', async ({ page }) => {
  await open(page);
  await node(page, FUNCTION).locator('circle.hit').click({ force: true });
  await expect(members(page)).toHaveCount(24);
  await expect(node(page, FUNCTION).locator('text')).toHaveText('Function · 24 of 30');
  await waitIdle(page);
  await node(page, FUNCTION).locator('circle.hit').click({ force: true });
  await expect(members(page)).toHaveCount(30);
  await expect(page.locator(`#graph g.lk[data-t="${FUNCTION}"][data-p="rdf:type"]`)).toHaveCount(30);
});

test('a drawn member can be selected and reads in plain names', async ({ page }) => {
  await open(page);
  await node(page, FUNCTION).locator('circle.hit').click({ force: true });
  await expect(members(page)).toHaveCount(24);
  await waitIdle(page);
  await node(page, DEMO + 'fn/handler_00').locator('circle.hit').click({ force: true });
  await expect(page.locator('#detail h2')).toHaveText('handler_00');
  await expect(page.locator('#detail .badge.kind')).toHaveText('Function');
  await expect(page.locator('#detail .group', { hasText: 'hasParameter' })).toContainText('request');
});

test('unlabelled entities are named by the end of their IRI', async ({ page }) => {
  await open(page);
  await page.locator('#search').fill('demo');
  await page.locator('#results .row[data-iri]').first().click();
  const imports = page.locator('#detail .group', { hasText: 'hasImport' });
  await expect(imports.locator('.item .lbl')).toHaveText(['json', 'os.path', 're', 'typing.Any']);
});

test('Browse lists every class and pages through its members by name', async ({ page }) => {
  await open(page);
  await page.locator('#tabs [data-tab=browse]').click();
  const fn = page.locator('#tab-browse .cls-row', { hasText: 'Function' });
  await expect(fn).toContainText('30');
  await fn.click();
  await expect(page.locator('#tab-browse .item')).toHaveCount(30);
  await page.locator('#browse-filter').fill('handler_1');
  await expect(page.locator('#tab-browse .item')).toHaveCount(10);
  await expect(page.locator('#tab-browse .item .lbl').first()).toHaveText('handler_10');
  await page.locator('#tab-browse .item').first().click();
  await expect(page.locator('#detail h2')).toHaveText('handler_10');
  await expect(node(page, DEMO + 'fn/handler_10')).toHaveCount(1);
});

test('header numbers and Stats describe this graph, not PostgreSQL', async ({ page }) => {
  await open(page);
  await expect(page.locator('#numbers')).toContainText('320');
  await expect(page.locator('#numbers')).toContainText('classes');
  await expect(page.locator('#numbers')).not.toContainText('SQL functions');
  await page.locator('#tabs [data-tab=stats]').click();
  await expect(page.locator('#tab-stats')).toContainText('Triples by predicate');
  await expect(page.locator('#query')).not.toHaveValue(/mayRaise/);
});
