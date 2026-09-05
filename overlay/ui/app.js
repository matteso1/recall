// Featherstorm panel. Renders PanelState pushed by the Rust side; calls commands on clicks.
const tauri = window.__TAURI__ || {};
const invoke = tauri.core ? tauri.core.invoke : async () => { throw new Error("not inside Tauri"); };
const listen = tauri.event ? tauri.event.listen : async () => {};

const $ = (id) => document.getElementById(id);
let state = null;
let lastFlashUntil = 0;

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

function renderPath(plan) {
  if (!plan || !plan.path.length) return "";
  const nextId = plan.next ? plan.next.id : null;
  const items = plan.path.map((p) => {
    const cls = ["it", p.owned ? "owned" : "", p.id === nextId ? "next" : ""].join(" ");
    const tag = p.tag ? ` <span class="tag">(${esc(p.tag)})</span>` : "";
    const title = [p.name, p.why ? "— " + p.why : ""].join(" ");
    return `<span class="${cls}" title="${esc(title)}">${esc(p.short)}${tag}</span>`;
  });
  return `<div class="path"><span class="k">PATH</span>${items.join('<span class="dot">·</span>')}</div>`;
}

function renderNext(plan) {
  const n = plan && plan.next;
  if (!n) return "";
  const comps = n.components
    .map((c, i) => {
      const tree = i === n.components.length - 1 ? "└" : "├";
      return `<div class="c ${c.owned ? "owned" : ""}"><span class="tree">${tree}</span><span class="tick">${c.owned ? "✓" : ""}</span>${esc(c.name)}<span class="ccost">${gold(c.cost)}</span></div>`;
    })
    .join("");
  let buy = "";
  if (n.buy_now && state && state.phase === "ingame") {
    const isWhole = n.buy_now.id === n.id;
    const verb = n.buy_now_affordable ? (isWhole ? "Complete now:" : "Buy now:") : "Save for:";
    buy = `<div class="buy ${n.buy_now_affordable ? "can" : ""}">${verb} <b>${esc(n.buy_now.name)}</b> (${gold(n.buy_now.cost)})</div>`;
  }
  return `<div class="next"><span class="label">NEXT</span><span class="name" title="${esc(n.name)}">${esc(n.name)}</span><span class="cost">${gold(n.remaining_cost)}</span></div>${buy}<div class="components">${comps}</div>`;
}

function renderSkill(plan, flash) {
  if (!plan || !plan.skill || !plan.skill.next) return "";
  const s = plan.skill;
  const flashing = flash && flash.until_ms > Date.now();
  const cls = ["skill", s.point_available ? "available" : "", flashing ? "flash" : ""].join(" ");
  const lv = s.levels ? `Q${s.levels[0]} W${s.levels[1]} E${s.levels[2]} R${s.levels[3]}` : "";
  const head = s.point_available ? "LVL UP →" : "next point";
  return `<div class="${cls}"><span>${head}</span><span class="key">${esc(s.next)}</span><span>(${esc(s.label)})</span><span class="small">${lv}</span></div>`;
}

function renderWhy(plan) {
  if (!plan || !plan.why || !plan.why.length) return "";
  const all = plan.why.join("\n");
  return `<div class="why" title="${esc(all)}"><span class="i">ⓘ</span><span class="txt">${esc(plan.why[0])}${plan.why.length > 1 ? ` <span class="small">(+${plan.why.length - 1})</span>` : ""}</span></div>`;
}

function button(id, label, status) {
  const cls = status && status.startsWith("error") ? "error" : status === "done" ? "done" : "";
  const disabled = status === "working" ? "disabled" : "";
  const text = status === "done" ? label + " ✓" : status === "working" ? label + "…" : label;
  return `<button class="act ${cls}" id="${id}" ${disabled} title="${esc(status || "")}">${text}</button>`;
}

function render(s) {
  state = s;
  $("pill").textContent = s.phase === "noclient" ? "no client" : s.phase === "champselect" ? "champ select" : s.phase;
  $("pill").className = "pill " + s.phase;
  $("panel").classList.toggle("collapsed", !!s.collapsed);
  $("collapse").textContent = s.collapsed ? "+" : "–";

  const plan = s.plan;
  const parts = [];
  if (s.phase === "noclient" || s.phase === "idle") {
    parts.push(`<div class="message">${esc(s.message || "")}</div>`);
    if (s.summoner) parts.push(`<div class="summoner">${esc(s.summoner)}${s.ddragon ? " · patch " + esc(s.ddragon) : ""}</div>`);
  } else if (s.phase === "champselect" || s.phase === "loading") {
    if (s.message) parts.push(`<div class="message">${esc(s.message)}</div>`);
    if (s.lobby && (s.lobby.enemies.length || s.lobby.allies.length)) {
      parts.push(`<div class="teams"><b>vs</b> ${esc(s.lobby.enemies.join(", ") || "…")}</div>`);
    }
    if (plan && s.supported) {
      if (plan.matchup) parts.push(`<div class="matchup">${esc(plan.matchup)}</div>`);
      parts.push(renderPath(plan));
      parts.push(renderWhy(plan));
      parts.push(`<div class="small">Start: ${esc(plan.start.map((i) => i.name).join(", "))} · ${esc(plan.runes_summary)} · ${esc(plan.spells.join(" + "))}</div>`);
      parts.push(`<div class="row">${button("btn-runes", "Runes", s.imports.runes)}${button("btn-spells", "Spells", s.imports.spells)}${button("btn-itemset", "Item set", s.imports.itemset)}</div>`);
    }
  } else if (s.phase === "ingame") {
    if (!s.supported) {
      parts.push(`<div class="message">${esc(s.message || "Unsupported champion")}</div>`);
    } else if (plan) {
      parts.push(renderNext(plan));
      parts.push(renderPath(plan));
      parts.push(renderSkill(plan, s.flash));
      parts.push(renderWhy(plan));
    }
    if (s.live) {
      parts.push(`<div class="small">${fmtTime(s.live.game_time)} · lvl ${s.live.level} · ${gold(s.live.gold)} · ${esc(s.live.kda)}</div>`);
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
