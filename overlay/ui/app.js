// An action-first view of the Rust decision. No item selection happens in JavaScript.
const tauri = window.__TAURI__ || {};
const invoke = tauri.core?.invoke || (async () => { throw new Error('Open Featherstorm to connect to League.'); });
const listen = tauri.event?.listen || (async () => {});
const $ = id => document.getElementById(id);
const DD = 'https://ddragon.leagueoflegends.com/cdn';
const SPELL_ICONS = {
  1: 'SummonerBoost.png', 3: 'SummonerExhaust.png', 4: 'SummonerFlash.png', 6: 'SummonerHaste.png',
  7: 'SummonerHeal.png', 11: 'SummonerSmite.png', 12: 'SummonerTeleport.png', 13: 'SummonerMana.png',
  14: 'SummonerDot.png', 21: 'SummonerBarrier.png', 32: 'SummonerSnowball.png',
};
let state = null;
let flashTimer = null;
let flashUntil = 0;
let controlChampion = null;
let renderedFresh = false;
let renderedSwiftplayFresh = false;
// Same six-second observation limit used by the shell's command validator.
const MAX_LIVE_AGE_MS = 6_000;
const busy = new Set();
const iconMaps = new Map();

function esc(value) {
  return String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}
function finite(value) { return typeof value === 'number' && Number.isFinite(value); }
function gold(value) { return finite(value) && value >= 0 ? `${Math.round(value).toLocaleString('en-US')}g` : 'price unavailable'; }
function fmtTime(value) {
  if (!finite(value)) return '—';
  const seconds = Math.max(0, Math.floor(value));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
function patch() { return /^\d+\.\d+\.\d+$/.test(state?.ddragon || '') ? state.ddragon : null; }
function itemIcon(id) { return patch() && Number.isInteger(id) ? `${DD}/${patch()}/img/item/${id}.png` : null; }
function spellIcon(id) { return patch() && SPELL_ICONS[id] ? `${DD}/${patch()}/img/spell/${SPELL_ICONS[id]}` : null; }
function runeIcon(id) {
  const path = iconMaps.get(patch())?.runes?.[id];
  return path ? `${DD}/img/${path}` : null;
}
function champIcon(name) {
  const id = iconMaps.get(patch())?.champions?.[String(name).toLowerCase().replace(/[^a-z0-9]/g, '')];
  return id ? `${DD}/${patch()}/img/champion/${id}.png` : null;
}
function icon(src, name, classes = '', owned = false) {
  const initials = String(name || '?').split(/\s+/).slice(0, 2).map(word => word[0]).join('');
  return `<span class="ico ${classes}${owned ? ' owned' : ''}" title="${esc(name)}" aria-label="${esc(name)}${owned ? ', owned' : ''}">
    <span class="symbol" aria-hidden="true">${esc(initials)}</span>${src ? `<img src="${esc(src)}" alt="" draggable="false">` : ''}${owned ? '<span class="check" aria-hidden="true">✓</span>' : ''}</span>`;
}
function html(id, value) {
  const node = $(id);
  if (node.dataset.rendered !== value) { node.innerHTML = value; node.dataset.rendered = value; }
}
function freshLive(s = state) {
  const source=s?.live_source;
  const age=finite(source?.observed_at_ms) ? Date.now()-source.observed_at_ms : NaN;
  return !!(s?.phase === 'ingame' && s.live && source?.stale === false && source?.identity_known === true
    && finite(source.age_ms) && source.age_ms<=MAX_LIVE_AGE_MS && age>=0 && age<=MAX_LIVE_AGE_MS);
}
function freshSwiftplay(s = state) {
  const observed = s?.swiftplay?.observed_at_ms;
  const age = finite(observed) ? Date.now() - observed : NaN;
  return s?.phase === 'swiftplay' && age >= 0 && age <= MAX_LIVE_AGE_MS;
}

// Once per observed patch, including failures: decorative images never block instructions
// or retry every poll. A patch change cannot mix two icon dictionaries.
async function loadIconMaps() {
  const version = patch();
  if (!version || iconMaps.has(version)) return;
  iconMaps.set(version, {});
  while (iconMaps.size > 2) iconMaps.delete(iconMaps.keys().next().value);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 8_000);
  try {
    const get = async name => {
      const response = await fetch(`${DD}/${version}/data/en_US/${name}.json`, { signal: controller.signal });
      if (!response.ok) throw new Error('Icon catalog unavailable');
      return response.json();
    };
    const [champions, runes] = await Promise.all([get('champion'), get('runesReforged')]);
    const maps = { champions: {}, runes: {} };
    for (const champion of Object.values(champions.data || {})) maps.champions[champion.name.toLowerCase().replace(/[^a-z0-9]/g, '')] = champion.id;
    for (const tree of runes) {
      maps.runes[tree.id] = tree.icon;
      for (const slot of tree.slots) for (const rune of slot.runes) maps.runes[rune.id] = rune.icon;
    }
    iconMaps.set(version, maps);
    if (patch() === version && state) render(state);
  } catch { /* Names remain readable without any image service. */ }
  finally { clearTimeout(timeout); }
}

function renderPath(plan) {
  if (!plan?.path?.length) return '';
  return `<div class="path" aria-label="Planned items"><span class="path-label">Build</span><ol>${plan.path.map(item => {
    const description = `${item.name}${item.why ? ` — ${item.why}` : ''}`;
    return `<li title="${esc(description)}">${icon(itemIcon(item.id), item.name, item.id === plan.next?.id ? 'next' : '', item.owned)}</li>`;
  }).join('')}</ol></div>`;
}
function renderNext(plan) {
  const next = plan?.next;
  if (!next) return `<div class="message">${esc(plan?.path?.length && plan.path.every(item => item.owned) ? 'Build complete. No automatic item sales.' : 'Waiting for a supported purchase recommendation.')}</div>`;
  if (!next.price_known || next.blocked || !next.buy_now) return `<div class="message warn">${esc(next.blocked || 'Waiting for a verified item price.')}</div><div class="target-only">Target: <strong>${esc(next.name)}</strong></div>`;
  const buy = next.buy_now;
  const affordable = next.buy_now_affordable;
  const toward = buy.id !== next.id ? `Toward ${next.name} · ${gold(next.remaining_cost)} left`
    : plan.learning?.kind==='start' ? 'Starting purchase' : `Complete ${next.name}`;
  const basket = affordable && next.basket?.length > 1
    ? `<div class="basket-next" title="${esc(next.basket.slice(1).map(item => `${item.name} ${gold(item.cost)}`).join(', '))}">Then ${esc(next.basket.slice(1).map(item => item.name).join(' + '))} · ${gold(next.basket_cost)} total</div>` : '';
  return `<div class="purchase ${affordable ? 'affordable' : 'saving'}">
    <div id="action-verb" class="action-verb">${affordable ? 'Recommended shop buy' : 'Save for'}</div>
    <div class="action-row">${icon(itemIcon(buy.id), buy.name, 'hero-icon')}
      <h1 id="action-name" title="${esc(buy.name)}">${esc(buy.name)}</h1><span id="action-cost">${gold(buy.cost)}</span>
    </div><div class="target-line" title="${esc(toward)}">${esc(toward)}</div>
    ${!affordable && finite(next.save_gap) ? `<div id="action-gap">${gold(next.save_gap)} more</div>` : ''}${basket}
  </div>`;
}
function renderWhy(plan) {
  const reason = plan?.learning?.reason || plan?.why?.[0];
  return reason ? `<p id="action-reason" title="${esc(reason)}">${esc(reason)}</p>` : '';
}
function renderSkill(plan, flash) {
  const skill = plan?.skill;
  if (!skill?.next) return '';
  const pulse = flash?.until_ms > Date.now();
  return `<div class="skill ${skill.point_available ? 'available' : ''}${pulse ? ' flash' : ''}"><span>${skill.point_available ? 'Level up' : 'Next point'}</span><kbd>${esc(skill.next)}</kbd><span class="skill-priority">${esc(skill.label)}</span></div>`;
}
function renderLoadout(plan) {
  const rune = plan.runes?.perks?.[0];
  const spells = (plan.spell_ids || []).map((id, i) => icon(spellIcon(id), plan.spells?.[i] || `Spell ${id}`)).join('');
  const starters = (plan.start || []).filter(item => item.id !== 3340).map(item => icon(itemIcon(item.id), item.name, '', item.owned)).join('');
  return `<div class="loadout" title="${esc(plan.runes_summary)}">${rune ? icon(runeIcon(rune), plan.runes_summary, 'round') : ''}${spells}<span class="sep"></span>${starters}</div>`;
}
function importButton(command, label, status) {
  return `<button data-command="${command}" ${state?.demo || status === 'working' || busy.has(command) ? 'disabled' : ''} class="${status === 'done' ? 'done' : status?.startsWith('error') ? 'error' : ''}" title="${esc(status || '')}">${label}${status === 'done' ? ' ✓' : status === 'working' ? '…' : ''}</button>`;
}
function renderSwiftplay(s) {
  const view = s.swiftplay;
  const slots = view?.slots || [];
  const fresh = freshSwiftplay(s);
  const ready = fresh && view?.ready && slots.length === 2 && slots.every(slot =>
    slot.plan?.path?.length && ['runes', 'spells', 'itemset'].every(key => ['done', 'off', 'kept'].includes(slot.imports?.[key])));
  const status = !fresh ? 'Waiting for fresh lobby data.' : ready ? 'Both choices ready'
    : view?.preparing ? 'Preparing your choices…' : 'Waiting for both loadouts.';
  const message = fresh ? view?.message : 'Readiness will update when the lobby reconnects.';
  return `<section class="swiftplay" aria-label="Swiftplay preparation">
    <h1>Your two choices</h1><p id="swiftplay-status" class="${ready ? 'swiftplay-ready' : !fresh ? 'warn' : 'hint'}">${esc(status)}</p>
    <ol class="swiftplay-slots" aria-label="Swiftplay choices">${slots.map(slot => `<li class="swiftplay-slot">
      <div class="swiftplay-choice"><h2 title="${esc(slot.champion)}">${esc(slot.champion)}</h2><span class="swiftplay-role">${esc(slot.position)}${slot.plan?.source_position ? ` · ${esc(slot.plan.source_position)} build` : ''}</span></div>
      <dl class="swiftplay-imports">${[['runes', 'Runes'], ['spells', 'Spells'], ['itemset', 'Build']].map(([key, label]) => {
        const value = slot.imports?.[key] || 'idle';
        const failed = value.startsWith('error');
        const text = !fresh ? 'Unverified' : failed ? 'Failed'
          : ({ done: 'Ready', working: 'Preparing', idle: 'Waiting', off: 'Off', kept: 'Kept' })[value] || 'Waiting';
        const tone = !fresh ? '' : failed ? 'swiftplay-error' : value === 'done' ? 'swiftplay-ready' : '';
        return `<div class="${tone}" title="${esc(fresh ? value : 'Waiting for a fresh lobby observation')}"><dt>${label}</dt> <dd>${text}</dd></div>`;
      }).join('')}</dl>
      ${fresh && slot.message ? `<p class="swiftplay-message hint" title="${esc(slot.message)}">${esc(slot.message)}</p>` : ''}
    </li>`).join('')}</ol>
    ${message ? `<p class="swiftplay-message hint" title="${esc(message)}">${esc(message)}</p>` : ''}
    <p class="hint">Your assigned champion appears when the game starts.</p>
  </section>`;
}
function renderMain(s) {
  const plan = s.plan;
  if (s.phase === 'swiftplay') return renderSwiftplay(s);
  if (s.phase === 'idle' || s.phase === 'noclient') return `<div class="empty-state"><h1>${s.phase === 'noclient' ? 'Ready when you are' : 'Ready for the next game'}</h1><p>${esc(s.message || 'Pick any champion. Your build will appear here.')}</p>${s.phase === 'noclient' ? '<p class="hint">Open League, pick a champion, and we’ll prepare your build.</p>' : ''}</div>`;
  if (s.phase === 'ingame' && !s.demo && !freshLive(s)) {
    const elapsed=finite(s.live_source?.observed_at_ms) ? Math.max(0,Date.now()-s.live_source.observed_at_ms) : 0;
    const age = finite(s.live_source?.age_ms) ? ` Last update ${Math.floor(Math.max(s.live_source.age_ms,elapsed) / 1000)}s ago.` : '';
    return `<div id="live-warning" class="message warn">Waiting for fresh game data.${esc(age)}</div><p class="hint">Purchase and skill advice is paused until your live state is confirmed.</p>${renderPath(plan)}`;
  }
  if (!s.supported || !plan?.path?.length) return `<div class="message warn">${esc(s.message || plan?.note || 'Loading this champion’s build data…')}</div><p class="hint">Only this champion’s own data is used; another champion’s build is never substituted.</p>`;
  // A same-champion fallback (no data for the assigned role) is said out loud, above the action.
  const fallback = plan.source_position
    ? `<p id="fallback-note" class="message warn" title="${esc(plan.note || '')}">${esc(plan.note || `No ${plan.position} data; using the ${plan.source_position} build as a starting point.`)}</p>` : '';
  if (s.phase === 'ingame') return fallback + renderNext(plan) + renderWhy(plan) + renderPath(plan) + renderSkill(plan, s.flash);
  const enemies = s.lobby?.enemies || [];
  return `<div class="pregame-heading">Your loadout is ready</div>${fallback}${plan.matchup ? `<p class="matchup">${esc(plan.matchup)}</p>` : ''}
    ${enemies.length ? `<div class="teams" aria-label="Enemy champions">${enemies.map(name => icon(champIcon(name), name)).join('')}</div>` : ''}
    ${renderPath(plan)}${renderLoadout(plan)}<div class="row import-row">${importButton('import_runes', 'Runes', s.imports?.runes)}${importButton('import_spells', 'Spells', s.imports?.spells)}${importButton('import_item_set', 'Item set', s.imports?.itemset)}</div>`;
}

function renderDetails(plan) {
  const tip = plan?.learning;
  html('lesson', tip ? `<h2>${esc(tip.title)}</h2><p>${esc(tip.reason)}</p><p>${esc(tip.lesson)}</p><p class="tradeoff">${esc(tip.tradeoff)}</p>` : '');
  const components = plan?.next?.components || [];
  const alternative = plan?.alternative;
  const evidence = plan?.core_evidence;
  const lines = [...(plan?.context || []), ...(plan?.warnings || [])];
  if (state.aggregate_source?.stale) lines.unshift('Using cached build data; the live inventory is checked separately.');
  html('details-context', `${components.length ? `<h2>Components toward this target</h2><ul class="component-list">${components.map(item => `<li>${icon(itemIcon(item.id), item.name, '', item.owned)}<span>${esc(item.name)}</span><span>${item.owned ? 'Owned' : gold(item.cost)}</span></li>`).join('')}</ul>` : ''}
    ${alternative ? `<h2>Closest alternative</h2><p><strong>${esc(alternative.item.name)}</strong>${finite(alternative.remaining_cost) ? ` · ${gold(alternative.remaining_cost)} remaining` : ''}<br>${esc(alternative.reason)}</p>` : ''}
    ${lines.length ? `<h2>What the planner knows</h2><ul class="context-list">${lines.map(line => `<li>${esc(line)}</li>`).join('')}</ul>` : ''}
    ${evidence ? `<h2>Build evidence</h2><p>${evidence.games.toLocaleString('en-US')} games using the popular core. ${evidence.interval ? `Observed win-rate range: ${(100 * evidence.interval.lower).toFixed(1)}–${(100 * evidence.interval.upper).toFixed(1)}% (95% interval).` : ''} This is not your chance of winning or proof that the items caused wins.</p>` : ''}
    ${plan?.source ? `<p class="source">${esc(plan.source)}</p>` : ''}`);
}
function controlsAvailable() {
  return !state?.demo && !!state?.supported && !!state.plan?.path?.length && (state.phase === 'champselect' || freshLive());
}
function renderControls() {
  const plan = state?.plan;
  const available = controlsAvailable();
  const pending = busy.has('set_build_preference') || busy.has('pin_item') || busy.has('clear_item_pin');
  for (const button of document.querySelectorAll('[data-mode]')) {
    button.disabled = !available || pending;
    button.setAttribute('aria-pressed', String(button.dataset.mode === (plan?.preferences?.mode || 'balanced')));
  }
  const select = $('target-select');
  const choices = [...(plan?.path || []), ...(plan?.options || [])].filter(item => !item.owned);
  const seen = new Set();
  const unique = choices.filter(item => !seen.has(item.id) && seen.add(item.id));
  const options = unique.map(item => `<option value="${item.id}">${esc(item.name)}</option>`).join('');
  if (select.dataset.rendered !== options && document.activeElement !== select) {
    const previous = controlChampion === state?.champion ? select.value : null;
    select.innerHTML = options;
    select.dataset.rendered = options;
    const chosen = [previous, plan?.preferences?.pinned_item, plan?.next?.id].find(id => unique.some(item => String(item.id) === String(id)));
    if (chosen != null) select.value = String(chosen);
  }
  controlChampion = state?.champion;
  select.disabled = !available || pending || !unique.length;
  $('pin-target').disabled = !available || pending || !unique.some(item => String(item.id) === select.value);
  $('auto-target').disabled = !available || pending;
  $('preference-label').textContent = plan?.preferences?.pinned_item ? 'Pinned' : plan?.preferences?.mode === 'survival' ? 'Protection' : 'Auto';
}
function renderRecap(recap) {
  const show = !!recap?.decisions?.length && ['idle', 'noclient'].includes(state.phase);
  $('recap').hidden = !show;
  if (!show) return;
  html('recap', `<h2>Last game · ${esc(recap.champion)}</h2><p class="hint">Your recent recommendations. Purchases are observations, not a performance grade.</p><ol class="recap-list">${recap.decisions.slice(-8).reverse().map(decision => `<li><div class="recap-heading"><time>${fmtTime(decision.game_time)}</time><strong>${esc(decision.buy_name || decision.target_name)}</strong></div><p>${esc(decision.reason)}</p><div class="feedback" aria-label="Was this recommendation useful?"><button data-feedback="useful" data-decision="${esc(decision.id)}" aria-pressed="${decision.feedback === 'useful'}">Useful</button><button data-feedback="not_useful" data-decision="${esc(decision.id)}" aria-pressed="${decision.feedback === 'not_useful'}">Not useful</button></div></li>`).join('')}</ol>`);
}

function render(s) {
  if (!s) return;
  state = s;
  renderedFresh = freshLive(s);
  renderedSwiftplayFresh = freshSwiftplay(s);
  loadIconMaps();
  $('pill-text').textContent = s.demo ? 'demo' : ({ noclient: 'no client', idle: 'ready', champselect: 'select', loading: 'loading', ingame: 'in game' })[s.phase] || s.phase;
  $('pill').className = `chip ${s.phase}`;
  $('panel').classList.toggle('collapsed', !!s.collapsed);
  $('collapse').textContent = s.collapsed ? '+' : '−';
  $('collapse').setAttribute('aria-label', s.collapsed ? 'Expand panel' : 'Collapse panel');
  $('ctx').textContent = s.phase !== 'swiftplay' && s.champion
    ? `${s.champion}${s.plan?.position ? ` · ${s.plan.position}` : ''}${s.plan?.source_position ? ` (${s.plan.source_position} build)` : ''}` : '';
  $('ctx').title = $('ctx').textContent;
  html('main-content', renderMain(s));
  $('learn-panel').hidden = !s.plan?.path?.length || ['idle', 'noclient', 'swiftplay'].includes(s.phase);
  renderDetails(s.plan);
  renderControls();
  renderRecap(s.recap);
  $('stats').hidden = s.phase !== 'ingame' || !s.live;
  if (s.live) html('stats', `<span>${fmtTime(s.live.game_time)}</span><span>Level ${esc(s.live.level)}</span><span>${gold(s.live.gold)}</span><span>${s.demo ? 'Demo' : freshLive(s) ? 'Live' : 'Paused'}</span>`);
  if (s.journal_error) setStatus(s.journal_error);
  if (s.flash?.until_ms && s.flash.until_ms !== flashUntil) {
    clearTimeout(flashTimer);
    flashUntil = s.flash.until_ms;
    flashTimer = setTimeout(() => { if (state) render(state); }, Math.max(0, flashUntil - Date.now()) + 50);
  }
}
function setStatus(message) {
  $('command-status').textContent = String(message || '');
  $('command-status').hidden = !message;
}
async function command(name, args, button) {
  if (busy.has(name)) return;
  busy.add(name);
  if (button) button.disabled = true;
  setStatus('');
  renderControls();
  try { await invoke(name, args); }
  catch (error) { setStatus(error?.message || error); }
  finally { busy.delete(name); if (button?.isConnected) button.disabled = false; renderControls(); }
}
document.addEventListener('error', event => { if (event.target instanceof HTMLImageElement) event.target.hidden = true; }, true);
document.addEventListener('click', event => {
  const button = event.target.closest('button');
  if (!button || button.disabled) return;
  if (button.dataset.command) command(button.dataset.command, undefined, button);
  if (button.dataset.mode) command('set_build_preference', { mode: button.dataset.mode }, button);
  if (button.dataset.feedback) command('rate_decision', { decisionId: button.dataset.decision, feedback: button.dataset.feedback }, button);
});
$('pin-target').addEventListener('click', () => command('pin_item', { itemId: Number($('target-select').value) }, $('pin-target')));
$('auto-target').addEventListener('click', () => command('clear_item_pin', undefined, $('auto-target')));
$('target-select').addEventListener('change', renderControls);
$('target-select').addEventListener('blur', renderControls);
$('collapse').addEventListener('click', () => command('set_collapsed', { collapsed: !state?.collapsed }, $('collapse')));
$('quit').addEventListener('click', () => command('quit', undefined, $('quit')));
listen('state', event => render(event.payload));
invoke('get_state').then(render).catch(error => { html('main-content', `<p class="message warn">${esc(error?.message || error)}</p>`); });
// The UI also expires advice when the entire state stream stalls. No continuous
// re-render, focus loss, network call, or animation is needed for this watchdog.
setInterval(() => {
  if ((state?.phase === 'ingame' && renderedFresh !== freshLive(state))
    || (state?.phase === 'swiftplay' && renderedSwiftplayFresh !== freshSwiftplay(state))) render(state);
}, 1_000);
