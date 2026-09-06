import { test, expect } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Real serialized production plans, not a parallel JS reconstruction of the planner.
const overlay = fileURLToPath(new URL('../../overlay/', import.meta.url));
const report = JSON.parse(execFileSync('cargo', ['run', '--quiet', '--locked', '-p', 'featherstorm-core', '--bin', 'replay', '--', '--fixtures', '--json'], {
  cwd: overlay, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024, timeout: 120_000,
}));
const records = report.records;
function fixture(champion = 'Xayah', scenario = 'synthetic_progression') {
  const record = records.find(record => record.champion === champion && record.scenario === scenario);
  if (!record) throw new Error(`Missing replay fixture: ${champion} / ${scenario}`);
  return {
    phase: 'ingame', gameflow: 'InProgress', champion,
    supported: true, plan: structuredClone(record.plan), imports: {},
    live: { game_time: record.game_time, gold: record.snapshot.gold, level: record.snapshot.level, kda: '0/0/0' },
    live_source: { observed_at_ms: Date.now(), age_ms: 0, stale: false, identity_known: true },
    aggregate_source: { observed_at_ms: Date.now(), age_ms: 0, stale: false, identity_known: true },
    ddragon: '16.17.1', collapsed: false, recap: null,
  };
}

async function openPanel(page, state) {
  // No Riot, CDN, local-game API, or account writes during tests.
  await page.route('https://**/*', route => route.abort());
  await page.addInitScript(initial => {
    window.testState = initial;
    window.calls = [];
    window.emitState = next => { window.testState = next; window.stateListener?.({ payload: next }); };
    window.__TAURI__ = {
      core: { invoke: async (command, args) => {
        window.calls.push({ command, args });
        if (command === 'get_state') return window.testState;
        if (window.commandError) throw window.commandError;
      } },
      event: { listen: async (event, callback) => { if (event === 'state') window.stateListener = callback; return () => {}; } },
    };
  }, state);
  await page.goto(new URL('../../overlay/ui/index.html', import.meta.url).href);
  await expect(page.locator('#pill-text')).toHaveText(state.demo ? 'demo' : state.phase === 'swiftplay' ? 'swiftplay' : 'in game');
}

function swiftplayFixture() {
  const state = fixture();
  return {
    ...state, phase: 'swiftplay', gameflow: 'Lobby', champion: null, plan: null,
    supported: false, live: null, live_source: null,
    swiftplay: {
      slots: [fixture('Xayah'), fixture('Ahri')].map((choice, index) => ({
        index, champion: choice.champion, position: index === 0 ? 'ADC' : 'MID',
        plan: choice.plan, imports: { runes: 'done', spells: 'done', itemset: 'done' }, message: null,
      })),
      observed_at_ms: Date.now(), ready: true, preparing: false,
      message: 'Both choices are prepared for queue.',
    },
  };
}

test('Swiftplay shows two independent loadouts before a champion is assigned', async ({ page }) => {
  await openPanel(page, swiftplayFixture());
  const slots = page.getByRole('list', { name: 'Swiftplay choices' }).getByRole('listitem');
  await expect(slots).toHaveCount(2);
  await expect(slots.nth(0)).toContainText('Xayah');
  await expect(slots.nth(0)).toContainText('ADC');
  await expect(slots.nth(1)).toContainText('Ahri');
  await expect(slots.nth(1)).toContainText('MID');
  for (const slot of [slots.nth(0), slots.nth(1)]) {
    await expect(slot).toContainText('Runes Ready');
    await expect(slot).toContainText('Spells Ready');
    await expect(slot).toContainText('Build Ready');
  }
  await expect(page.locator('#swiftplay-status')).toContainText('Both choices ready');
  await expect(page.locator('#ctx')).toBeEmpty();
  await expect(page.locator('#action-name')).toHaveCount(0);
  await expect(page.locator('#learn-panel')).toBeHidden();
  await expect(page.locator('#stats')).toBeHidden();
  await expect(page.locator('#main-content button')).toHaveCount(0);
  await page.screenshot({ path: test.info().outputPath('swiftplay-ready.png') });
});

test('Swiftplay does not call both choices ready while one loadout is still preparing', async ({ page }) => {
  const state = swiftplayFixture();
  state.swiftplay.ready = false;
  state.swiftplay.preparing = true;
  state.swiftplay.message = 'Preparing your second choice.';
  state.swiftplay.slots[1].plan = null;
  state.swiftplay.slots[1].imports = { runes: 'working', spells: 'idle', itemset: 'error: Build data unavailable' };
  state.swiftplay.slots[1].message = 'Build data unavailable. Retrying automatically.';
  await openPanel(page, state);
  const slots = page.getByRole('list', { name: 'Swiftplay choices' }).getByRole('listitem');
  await expect(slots.nth(0)).toContainText('Runes Ready');
  await expect(slots.nth(1)).toContainText('Runes Preparing');
  await expect(slots.nth(1)).toContainText('Spells Waiting');
  await expect(slots.nth(1)).toContainText('Build Failed');
  await expect(slots.nth(1)).toContainText('Retrying automatically');
  await expect(page.locator('#swiftplay-status')).not.toContainText('Both choices ready');
  await expect(page.locator('#main-content')).toContainText('Preparing your second choice');
});

test('Swiftplay readiness expires when lobby updates stop', async ({ page }) => {
  const state = swiftplayFixture();
  await page.clock.install({ time: Date.now() });
  await openPanel(page, state);
  await expect(page.locator('#swiftplay-status')).toContainText('Both choices ready');
  await page.clock.fastForward(8_000);
  await expect(page.locator('#swiftplay-status')).toContainText('Waiting for fresh lobby data');
  await expect(page.locator('#main-content')).not.toContainText('Ready');
  await expect(page.locator('#main-content')).not.toContainText('prepared for queue');
  await expect(page.getByRole('list', { name: 'Swiftplay choices' }).getByRole('listitem')).toHaveCount(2);
});

test('Swiftplay rejects an already expired lobby observation', async ({ page }) => {
  const state = swiftplayFixture();
  state.swiftplay.observed_at_ms -= 7_000;
  await openPanel(page, state);
  await expect(page.locator('#swiftplay-status')).toContainText('Waiting for fresh lobby data');
  await expect(page.locator('#main-content')).not.toContainText('Ready');
});

test('Swiftplay keeps the same champion in different roles as separate choices', async ({ page }) => {
  const state = swiftplayFixture();
  state.swiftplay.slots[1] = { ...state.swiftplay.slots[0], index: 1, position: 'MID' };
  await openPanel(page, state);
  const slots = page.getByRole('list', { name: 'Swiftplay choices' }).getByRole('listitem');
  await expect(slots).toHaveCount(2);
  await expect(slots.nth(0)).toContainText('Xayah');
  await expect(slots.nth(0)).toContainText('ADC');
  await expect(slots.nth(1)).toContainText('Xayah');
  await expect(slots.nth(1)).toContainText('MID');
});

test('Swiftplay hands off to only the assigned champion when the game starts', async ({ page }) => {
  await openPanel(page, swiftplayFixture());
  await expect(page.getByRole('list', { name: 'Swiftplay choices' })).toBeVisible();
  const assigned = fixture('Ahri');
  await page.evaluate(next => window.emitState(next), assigned);
  await expect(page.locator('#pill-text')).toHaveText('in game');
  await expect(page.locator('#ctx')).toContainText('Ahri');
  await expect(page.locator('#action-name')).toHaveText(assigned.plan.next.buy_now.name);
  await expect(page.getByRole('list', { name: 'Swiftplay choices' })).toHaveCount(0);
  await expect(page.locator('#main-content')).not.toContainText('Xayah');
});

test('Swiftplay stays within a narrow panel with long names and an import failure', async ({ page }) => {
  const state = swiftplayFixture();
  state.swiftplay.ready = false;
  state.swiftplay.preparing = true;
  state.swiftplay.message = 'Preparing your selected loadouts.';
  state.swiftplay.slots[0].champion = 'A very long champion name with <unsafe> markup';
  state.swiftplay.slots[0].position = 'SUPPORT';
  state.swiftplay.slots[0].imports.runes = 'error: Rune page unavailable';
  state.swiftplay.slots[0].message = 'Rune page unavailable. Retrying automatically.';
  await page.setViewportSize({ width: 320, height: 300 });
  await openPanel(page, state);
  const slots = page.getByRole('list', { name: 'Swiftplay choices' }).getByRole('listitem');
  await expect(slots.nth(0)).toContainText(state.swiftplay.slots[0].champion);
  await expect(slots.nth(1)).toContainText('Ahri');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  expect(await page.locator('#body').evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
  const bounds = await slots.nth(1).boundingBox();
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(300);
  await expect(page.locator('unsafe')).toHaveCount(0);
  await page.screenshot({ path: test.info().outputPath('swiftplay-narrow.png') });
});

test('one purchase is primary and explanation is optional', async ({ page }) => {
  const state = fixture();
  await openPanel(page, state);
  await expect(page.locator('#action-name')).toHaveText(state.plan.next.buy_now.name);
  await expect(page.locator('#action-verb')).toHaveText(state.plan.next.buy_now_affordable ? 'Recommended shop buy' : 'Save for');
  await expect(page.locator('#action-reason')).toHaveText(state.plan.learning.reason);
  await expect(page.locator('#learn-panel')).not.toHaveAttribute('open');
  await expect(page.locator('#lesson')).toBeHidden();
  await expect(page.locator('#target-select')).toBeHidden();
  const bounds = await page.locator('#learn-panel > summary').boundingBox();
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(300);
  await page.screenshot({path:test.info().outputPath('action-panel.png')});
});

test('optional controls survive polling and send the exact local commands', async ({ page }) => {
  const state = fixture('Ahri');
  await openPanel(page, state);
  await page.locator('#learn-panel > summary').click();
  await page.getByRole('button', { name: 'More protection', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.calls.at(-1))).toEqual({ command: 'set_build_preference', args: { mode: 'survival' } });
  const select = page.locator('#target-select');
  const target = state.plan.options[0] ?? state.plan.path.find(item => !item.owned);
  await select.selectOption(String(target.id));
  await select.focus();
  await page.evaluate(next => window.emitState(next), { ...state, live: { ...state.live, gold: state.live.gold + 3 } });
  await expect(page.locator('#learn-panel')).toHaveAttribute('open');
  await expect(select).toBeFocused();
  await expect(select).toHaveValue(String(target.id));
  await page.getByRole('button', { name: 'Pin target', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.calls.at(-1))).toEqual({ command: 'pin_item', args: { itemId: target.id } });
  await page.getByRole('button', { name: 'Auto', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.calls.at(-1).command)).toBe('clear_item_pin');
});

test('stale or unknown live identity cannot display a fresh buy instruction', async ({ page }) => {
  const state = fixture();
  await openPanel(page, state);
  await page.evaluate(next => window.emitState(next), { ...state, live_source: { ...state.live_source, stale: true, age_ms: 12_000 } });
  await expect(page.locator('#live-warning')).toContainText('Waiting for fresh game data');
  await expect(page.locator('#action-name')).toHaveCount(0);
  await expect(page.locator('.skill.available')).toHaveCount(0);
  await page.locator('#learn-panel > summary').click();
  await expect(page.getByRole('button', { name: 'Pin target', exact: true })).toBeDisabled();
});

test('missing item price is never presented as a free purchase', async ({ page }) => {
  const state = fixture();
  state.plan.next.price_known = false;
  state.plan.next.remaining_cost = 0;
  state.plan.next.buy_now = null;
  state.plan.next.blocked = 'Item price is unavailable';
  await openPanel(page, state);
  await expect(page.locator('#main-content')).toContainText('Item price is unavailable');
  await expect(page.locator('#action-name')).toHaveCount(0);
  await expect(page.locator('#main-content')).not.toContainText('0g');
});

test('legal saving advice shows the missing gold, not a buy-now claim', async ({ page }) => {
  const state = fixture('Ornn');
  state.plan.next.buy_now_affordable = false;
  state.plan.next.buy_now = { id: 1028, name: 'Ruby Crystal', cost: 400, owned: false };
  state.plan.next.save_gap = 150;
  await openPanel(page, state);
  await expect(page.locator('#action-verb')).toHaveText('Save for');
  await expect(page.locator('#action-gap')).toHaveText('150g more');
  await expect(page.locator('#main-content')).not.toContainText('Recommended shop buy');
});

test('command failures are readable without replacing the recommendation', async ({ page }) => {
  const state = fixture();
  await openPanel(page, state);
  await page.locator('#learn-panel > summary').click();
  await page.evaluate(() => { window.commandError = 'Waiting for fresh game data'; });
  await page.getByRole('button', { name: 'More protection', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('Waiting for fresh game data');
  await expect(page.locator('#action-name')).toBeVisible();
  await expect(page.getByRole('button', { name: 'More protection', exact: true })).toBeEnabled();
});

test('long names and narrow viewports never create horizontal overflow', async ({ page }) => {
  const state = fixture();
  state.plan.next.buy_now.name = 'A very long localized item name with <unsafe> markup';
  await page.setViewportSize({ width: 320, height: 300 });
  await openPanel(page, state);
  await expect(page.locator('#action-name')).toHaveText(state.plan.next.buy_now.name);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  expect(await page.locator('unsafe').count()).toBe(0);
  await page.locator('#learn-panel > summary').click();
  expect(await page.locator('#body').evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
});

test('post-game recap records feedback without grading the player', async ({ page }) => {
  const state = fixture();
  await openPanel(page, state);
  await page.evaluate(next => window.emitState(next), {
    ...state, phase: 'idle', plan: null, live: null, message: 'Ready for the next game',
    recap: { session_id: 'test-session', champion: 'Xayah', role: 'ADC', patch: '16.17', engine_version: '2.0',
      source: 'op.gg', end_game_time: 1800, purchases: [], decisions: [{ id: 'decision-1', game_time: 740,
        target_id: 3031, target_name: 'Infinity Edge', buy_name: 'Infinity Edge', buy_id: 3031,
        buy_affordable: true, reason: 'IE: only 725g left with your components',
        lesson: 'Finish an affordable upgrade before starting a different item.', kind: 'completion',
        remaining_cost: 725, gold: 750, feedback: null }] },
  });
  await expect(page.locator('#recap')).toContainText('Last game');
  await expect(page.locator('#recap')).toContainText('Infinity Edge');
  await page.getByRole('button', { name: 'Useful', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.calls.at(-1))).toEqual({ command: 'rate_decision', args: { decisionId: 'decision-1', feedback: 'useful' } });
  await expect(page.locator('#recap')).not.toContainText('win probability');
});

test('keyboard access and reduced motion remain usable during updates', async ({ page }) => {
  const state = fixture();
  state.flash = {skill:'R', until_ms:Date.now()+60_000};
  await openPanel(page, state);
  const summary=page.locator('#learn-panel > summary');
  await summary.focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#lesson')).toBeVisible();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button',{name:'Balanced',exact:true})).toBeFocused();
  expect(await page.locator('.skill kbd').evaluate(node=>getComputedStyle(node).animationName)).toBe('none');
});

test('a stalled state stream expires the last buy instruction on its own', async ({ page }) => {
  const state=fixture();
  await page.clock.install({time:Date.now()});
  await openPanel(page,state);
  await expect(page.locator('#action-name')).toBeVisible();
  await page.clock.fastForward(8_000);
  await expect(page.locator('#live-warning')).toContainText('Waiting for fresh game data');
  await expect(page.locator('#action-name')).toHaveCount(0);
});

test('an explicit offline demo stays labeled and cannot import or change a live plan', async ({ page }) => {
  const state = { ...fixture(), demo: true, live_source: null };
  await openPanel(page, state);
  await expect(page.locator('#action-name')).toBeVisible();
  await expect(page.locator('#stats')).toContainText('Demo');
  await page.locator('#learn-panel > summary').click();
  await expect(page.getByRole('button', { name: 'More protection', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Pin target', exact: true })).toBeDisabled();
  await page.evaluate(next => window.emitState(next), { ...state, phase: 'champselect' });
  await expect(page.getByRole('button', { name: 'Runes', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Item set', exact: true })).toBeDisabled();
});

test('all real role fixtures render a single action without script errors', async ({ page }) => {
  const errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  await openPanel(page,fixture());
  for (const champion of ['Ahri','Ornn','Darius','Lulu','Lee Sin','Aphelios','Udyr']) {
    const state=fixture(champion);
    await page.evaluate(next=>window.emitState(next),state);
    if (state.plan.next?.buy_now && !state.plan.next.blocked) {
      await expect(page.locator('#action-name')).toHaveText(state.plan.next.buy_now.name);
    }
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  }
  expect(errors).toEqual([]);
});
