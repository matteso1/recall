// Featherstorm panel. Renders the PanelState pushed by the Rust side; calls commands on clicks.
// Icons come from Data Dragon (items, champions, spells, runes) at the patch the brain loaded.
const tauri = window.__TAURI__ || {};
const invoke = tauri.core ? tauri.core.invoke : async () => { throw new Error("not inside Tauri"); };
const listen = tauri.event ? tauri.event.listen : async () => {};

const DD = "https://ddragon.leagueoflegends.com/cdn";
const SPELL_ICONS = {
  1: "SummonerBoost.png", 3: "SummonerExhaust.png", 4: "SummonerFlash.png", 6: "SummonerHaste.png", 7: "SummonerHeal.png",
  11: "SummonerSmite.png", 12: "SummonerTeleport.png", 13: "SummonerMana.png", 14: "SummonerDot.png", 21: "SummonerBarrier.png",
  30: "SummonerPoroRecall.png", 31: "SummonerPoroThrow.png", 32: "SummonerSnowball.png",
};
const TRINKET = 3340;

const $ = (id) => document.getElementById(id);
let state = null;
let lastFlashUntil = 0;
let champIds = null;   // champion display name -> Data Dragon id ("MonkeyKing"), fetched once
let runeIcons = null;  // perk / style id -> icon path, fetched once
let loadingIcons = false;

function esc(s) {
  return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}
function gold(n) {
  return Math.round(n).toLocaleString("en-US") + "g";
}
function fmtTime(s) {
  s = Math.max(0, Math.floor(s));
  return String(Math.floor(s / 60)).padStart(2, "0") + ":" + String(s % 60).padStart(2, "0");
}
function ver() {
  return (state && state.ddragon) || "16.17.1";
}
function itemIcon(id) {
  return `${DD}/${ver()}/img/item/${id}.png`;
}
function champIcon(name) {
  const id = champIds && champIds[name.toLowerCase().replace(/[^a-z0-9]/g, "")];
  return id ? `${DD}/${ver()}/img/champion/${id}.png` : null;
}
function spellIcon(id) {
  return SPELL_ICONS[id] ? `${DD}/${ver()}/img/spell/${SPELL_ICONS[id]}` : null;
}
function runeIcon(id) {
  return runeIcons && runeIcons[id] ? `${DD}/img/${runeIcons[id]}` : null;
}
function ico(src, cls, title, inner) {
  const img = src ? `<img src="${src}" alt="" draggable="false">` : "";
  return `<span class="ico ${cls || ""}" title="${esc(title || "")}">${img}${inner || ""}</span>`;
}

// Champion ids and rune icons are the two things ids alone cannot draw; fetch them once per patch.
async function loadIconMaps() {
  if (loadingIcons || (champIds && runeIcons)) return;
  loadingIcons = true;
  try {
    const [champs, runes] = await Promise.all([
      fetch(`${DD}/${ver()}/data/en_US/champion.json`).then((r) => r.json()),
      fetch(`${DD}/${ver()}/data/en_US/runesReforged.json`).then((r) => r.json()),
    ]);
    champIds = {};
    for (const c of Object.values(champs.data)) champIds[c.name.toLowerCase().replace(/[^a-z0-9]/g, "")] = c.id;
    runeIcons = {};
    for (const tree of runes) {
      runeIcons[tree.id] = tree.icon;
      for (const slot of tree.slots) for (const r of slot.runes) runeIcons[r.id] = r.icon;
    }
    if (state) render(state);
  } catch (e) {
    console.warn("icon maps", e);
  } finally {
    loadingIcons = false;
  }
}

function renderNext(plan) {
  const n = plan && plan.next;
  if (!n) return "";
  let buy = "";
  if (n.buy_now && state && state.phase === "ingame") {
    const isWhole = n.buy_now.id === n.id;
    const verb = n.buy_now_affordable ? (isWhole ? "Complete now" : "Buy now") : "Save for";
    buy = `<div class="buy ${n.buy_now_affordable ? "can" : ""}">${verb}: <b>${esc(n.buy_now.name)}</b> · ${gold(n.buy_now.cost)}</div>`;
  }
  const comps = n.components
    .map((c) => `<span class="comp ${c.owned ? "owned" : ""}">${ico(itemIcon(c.id), c.owned ? "owned" : "", c.name)}${esc(c.name)} <span class="c-cost">${gold(c.cost)}</span></span>`)
    .join("");
  return `<div class="eyebrow">Next</div>
    <div class="next">${ico(itemIcon(n.id), "hot", n.name)}<div class="name" title="${esc(n.name)}">${esc(n.name)}</div><div class="cost">${gold(n.remaining_cost)}</div>${buy}</div>
    <div class="components">${comps}</div>`;
}

function renderPath(plan, withLabel) {
  if (!plan || !plan.path.length) return "";
  const nextId = plan.next ? plan.next.id : null;
  const tagged = plan.path.some((p) => p.tag);
  const slots = plan.path
    .map((p) => {
      const cls = [p.owned ? "owned" : "", p.id === nextId ? "next" : ""].join(" ");
      const title = p.name + (p.why ? " — " + p.why : "");
      const tag = p.tag ? `<span class="tag">${esc(p.tag)}</span>` : "";
      return ico(itemIcon(p.id), cls, title, tag);
    })
    .join("");
  return `<div class="path ${tagged ? "tagged" : ""}">${withLabel ? '<span class="eyebrow">Path</span>' : ""}<div class="slots">${slots}</div></div>`;
}

function renderSkill(plan, flash) {
  if (!plan || !plan.skill || !plan.skill.next) return "";
  const s = plan.skill;
  const flashing = flash && flash.until_ms > Date.now();
  const cls = ["skill", s.point_available ? "available" : "", flashing ? "flash" : ""].join(" ");
  const lv = s.levels ? `Q${s.levels[0]} W${s.levels[1]} E${s.levels[2]} R${s.levels[3]}` : "";
  const head = s.point_available ? "Level up" : "Next point";
  return `<div class="${cls}"><span>${head}</span><span class="key">${esc(s.next)}</span><span>${esc(s.label)}</span><span class="levels">${lv}</span></div>`;
}

function renderWhy(plan) {
  if (!plan || !plan.why || !plan.why.length) return "";
  const all = plan.why.join("\n");
  const more = plan.why.length > 1 ? ` <span class="more">+${plan.why.length - 1}</span>` : "";
  return `<div class="why" title="${esc(all)}"><span class="i">i</span><span class="txt">${esc(plan.why[0])}</span>${more}</div>`;
}

function renderTeams(lobby, plan) {
  if (!lobby || !lobby.enemies.length) return "";
  const lane = plan && plan.matchup_champion;
  const icons = lobby.enemies
    .map((name) => ico(champIcon(name), name === lane ? "lane" : "", name))
    .join("");
  // Names only when no matchup card names the lane opponent; the icons carry the rest on hover.
  const names = plan && plan.matchup ? "" : `<span class="names">${esc(lobby.enemies.join(", "))}</span>`;
  return `<div class="teams"><span class="eyebrow">vs</span>${icons}${names}</div>`;
}

function renderLoadout(plan) {
  const parts = [];
  if (plan.runes && plan.runes.perks && plan.runes.perks.length) {
    const keystone = plan.runes.perks[0];
    parts.push(ico(runeIcon(keystone), "keystone round", plan.runes_summary));
    parts.push(ico(runeIcon(plan.runes.sub_style), "tree round", plan.runes_summary));
  }
  if (plan.spell_ids && plan.spell_ids.length) {
    parts.push('<span class="sep"></span>');
    plan.spell_ids.forEach((id, i) => parts.push(ico(spellIcon(id), "spell", plan.spells[i] || "")));
  }
  const start = plan.start.filter((i) => i.id !== TRINKET);
  if (start.length) {
    parts.push('<span class="sep"></span>');
    start.forEach((i) => parts.push(ico(itemIcon(i.id), "start", i.name)));
  }
  return `<div class="loadout">${parts.join("")}</div>`;
}

function button(id, label, status) {
  const cls = status && status.startsWith("error") ? "error" : status === "done" ? "done" : "";
  const disabled = status === "working" ? "disabled" : "";
  const text = status === "done" ? label + " ✓" : status === "working" ? label + "…" : label;
  return `<button class="act ${cls}" id="${id}" ${disabled} title="${esc(status || "")}">${text}</button>`;
}

function render(s) {
  state = s;
  if (s.ddragon && !(champIds && runeIcons)) loadIconMaps();

  const pill = { noclient: "no client", idle: "ready", champselect: "champ select", loading: "loading", ingame: "in game" }[s.phase] || s.phase;
  $("pill-text").textContent = pill;
  $("pill").className = "chip " + s.phase;
  $("panel").classList.toggle("collapsed", !!s.collapsed);
  $("collapse").textContent = s.collapsed ? "+" : "–";
  const plan = s.plan;
  let ctx = "";
  if (s.champion && (s.phase === "champselect" || s.phase === "loading" || s.phase === "ingame")) {
    ctx = `<b>${esc(s.champion)}</b>${plan && plan.position ? " · " + esc(plan.position) : ""}`;
  } else if (s.summoner) {
    ctx = esc(s.summoner);
  }
  $("ctx").innerHTML = ctx;

  const parts = [];
  if (s.phase === "noclient" || s.phase === "idle") {
    parts.push(`<div class="message">${esc(s.message || "")}</div>`);
    if (s.summoner) parts.push(`<div class="summoner">${esc(s.summoner)}${s.ddragon ? " · patch " + esc(s.ddragon) : ""}</div>`);
  } else if (s.phase === "champselect" || s.phase === "loading") {
    if (s.message) parts.push(`<div class="message ${s.supported ? "" : "warn"}">${esc(s.message)}</div>`);
    parts.push(renderTeams(s.lobby, plan));
    if (plan && s.supported) {
      if (plan.note) parts.push(`<div class="message warn">${esc(plan.note)}</div>`);
      if (plan.matchup) parts.push(`<div class="matchup">${esc(plan.matchup)}</div>`);
      parts.push(renderPath(plan, true));
      parts.push(renderWhy(plan));
      parts.push(renderLoadout(plan));
      parts.push(`<div class="row">${button("btn-runes", "Runes", s.imports.runes)}${button("btn-spells", "Spells", s.imports.spells)}${button("btn-itemset", "Item set", s.imports.itemset)}</div>`);
      if (plan.source) parts.push(`<div class="source">${esc(plan.source)}</div>`);
    }
  } else if (s.phase === "ingame") {
    if (!s.supported) {
      parts.push(`<div class="message warn">${esc(s.message || "No build data for this champion")}</div>`);
    } else if (plan) {
      parts.push(renderNext(plan));
      parts.push(renderPath(plan, true));
      parts.push(renderSkill(plan, s.flash));
      parts.push(renderWhy(plan));
    }
    if (s.live) {
      parts.push(`<div class="stats"><span><b>${fmtTime(s.live.game_time)}</b></span><span>lvl <b>${s.live.level}</b></span><span><b>${gold(s.live.gold)}</b></span><span>${esc(s.live.kda)}</span></div>`);
    }
  }
  $("body").innerHTML = parts.join("");

  const wire = (id, cmd) => {
    const el = $(id);
    if (!el) return;
    el.onclick = () => {
      el.disabled = true;
      invoke(cmd).then((msg) => console.log(msg)).catch((err) => console.warn(err));
    };
  };
  wire("btn-runes", "import_runes");
  wire("btn-spells", "import_spells");
  wire("btn-itemset", "import_item_set");

  if (s.flash && s.flash.until_ms !== lastFlashUntil) {
    lastFlashUntil = s.flash.until_ms;
    setTimeout(() => { if (state) render(state); }, Math.max(0, s.flash.until_ms - Date.now()) + 50);
  }
}

$("collapse").onclick = () => invoke("set_collapsed", { collapsed: !(state && state.collapsed) }).catch(console.warn);
$("quit").onclick = () => invoke("quit").catch(console.warn);

listen("state", (e) => render(e.payload));
invoke("get_state").then(render).catch((err) => {
  $("message").textContent = "Not running inside Featherstorm: " + err;
});
