//! Imports into the client (runes, summoner spells, item set) and the commands the webview invokes.
//! The poller calls the `do_*` functions for auto-import; the buttons call the same code.
use crate::{controller, App};
use recall_core::engine::{BuildPreference, Plan};
use recall_core::itemset;
use recall_core::journal::Feedback;
use recall_core::lcu::Lcu;
use recall_core::runes;
use recall_core::session;
use recall_core::state::PanelState;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

fn set_import(app: &AppHandle, st: &App, which: &str, status: &str) {
    if status != "working" {
        log::info!("import {which}: {status}");
    }
    let status = status.to_string();
    let which = which.to_string();
    st.update(app, |p| match which.as_str() {
        "itemset" => p.imports.itemset = status,
        "runes" => p.imports.runes = status,
        _ => p.imports.spells = status,
    });
}

fn finish(app: &AppHandle, st: &App, which: &str, result: &Result<String, String>) {
    match result {
        Ok(_) => set_import(app, st, which, "done"),
        Err(e) => set_import(app, st, which, &format!("error: {e}")),
    }
}

#[derive(Clone, Copy)]
enum ImportKind {
    ItemSet,
    Runes,
    Spells,
}

fn signature(plan: &Plan, kind: ImportKind) -> String {
    match kind {
        ImportKind::ItemSet => format!(
            "{:?}|{:?}|{:?}",
            plan.start.iter().map(|item| item.id).collect::<Vec<_>>(),
            plan.path.iter().map(|item| item.id).collect::<Vec<_>>(),
            plan.options.iter().map(|item| item.id).collect::<Vec<_>>()
        ),
        ImportKind::Runes => plan
            .runes
            .as_ref()
            .map(|page| format!("{}:{}:{:?}", page.primary_style, page.sub_style, page.perks))
            .unwrap_or_default(),
        ImportKind::Spells => format!("{:?}", plan.spell_ids),
    }
}

/// Async imports carry an exact match/champion/loadout identity, not just "latest plan".
struct ImportGuard {
    generation: u64,
    champion: String,
    phase: String,
    kind: ImportKind,
    signature: String,
}

impl ImportGuard {
    fn capture(st: &App, plan: &Plan, kind: ImportKind) -> Result<Self, String> {
        let _planning = st.planning.lock().unwrap();
        if plan.path.is_empty() {
            return Err("no supported build data to import".into());
        }
        let guard = Self {
            generation: st.session.lock().unwrap().generation,
            champion: plan.champion.clone(),
            phase: st.snapshot().phase,
            kind,
            signature: signature(plan, kind),
        };
        guard.check_locked(st)?;
        Ok(guard)
    }

    fn check_locked(&self, st: &App) -> Result<(), String> {
        let panel = st.snapshot();
        let current = st.plan.lock().unwrap().clone();
        if st.session.lock().unwrap().generation != self.generation
            || panel.phase != self.phase
            || panel.champion.as_deref() != Some(self.champion.as_str())
            || !current.as_ref().is_some_and(|plan| {
                plan.champion == self.champion
                    && !plan.path.is_empty()
                    && signature(plan, self.kind) == self.signature
            })
        {
            return Err(
                "Match, champion, or recommended loadout changed before import completed".into(),
            );
        }
        match self.phase.as_str() {
            "champselect" => {
                let observed = *st.lobby_observed_at_ms.lock().unwrap();
                let own_identity = st
                    .lobby
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|lobby| lobby.my_cell >= 0 && lobby.my_champion > 0);
                if !own_identity
                    || !observed
                        .and_then(|at| controller::now_ms().checked_sub(at))
                        .is_some_and(|age| age <= 6000)
                {
                    return Err("Champion select data is stale; import will retry after a fresh observation".into());
                }
            }
            "ingame" if matches!(self.kind, ImportKind::ItemSet) => {
                if !panel
                    .live_source
                    .as_ref()
                    .is_some_and(|source| session::fresh_identity_at(source, controller::now_ms()))
                {
                    return Err("Live data is stale; item-set import is paused".into());
                }
            }
            _ => return Err("Runes and spells can only be imported during champion select".into()),
        }
        Ok(())
    }

    fn check(&self, st: &App) -> Result<(), String> {
        let _planning = st.planning.lock().unwrap();
        self.check_locked(st)
    }

    fn working(&self, app: &AppHandle, st: &App, which: &str) -> Result<(), String> {
        let _planning = st.planning.lock().unwrap();
        self.check_locked(st)?;
        set_import(app, st, which, "working");
        Ok(())
    }

    fn finish(
        &self,
        app: &AppHandle,
        st: &App,
        which: &str,
        result: Result<String, String>,
    ) -> Result<String, String> {
        let _planning = st.planning.lock().unwrap();
        self.check_locked(st)?; // An old task must not mark the new match's panel as done/error.
        finish(app, st, which, &result);
        result
    }
}

async fn import_rune_page(
    lcu: &Lcu,
    guard: &ImportGuard,
    st: &App,
    page: &Value,
) -> Result<(), String> {
    let pages = lcu.perk_pages().await.map_err(|error| error.to_string())?;
    let all_pages = pages
        .as_array()
        .ok_or("Client rune-page list is unavailable")?;
    let name = page
        .get("name")
        .and_then(Value::as_str)
        .ok_or("Rune page name is unavailable")?;
    if let Some(id) = runes::replacement_page_id(&pages, name) {
        guard.check(st)?;
        lcu.update_perk_page(id, page)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let owned = lcu
        .perk_inventory()
        .await
        .ok()
        .and_then(|inventory| inventory.get("ownedPageCount").and_then(Value::as_u64))
        .unwrap_or(2) as usize;
    let used = all_pages
        .iter()
        .filter(|page| page.get("isDeletable").and_then(Value::as_bool) == Some(true))
        .count();
    if used >= owned {
        return Err(format!(
            "all {owned} rune pages are in use; delete one in the client and retry"
        ));
    }
    guard.check(st)?;
    lcu.create_perk_page(page)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn do_import_item_set(app: &AppHandle, st: &Arc<App>) -> Result<String, String> {
    let plan = st
        .plan
        .lock()
        .unwrap()
        .clone()
        .ok_or("no build yet - wait for champ select")?;
    do_import_item_set_for_plan(app, st, plan).await
}

pub async fn do_import_item_set_for_plan(
    app: &AppHandle,
    st: &Arc<App>,
    plan: Plan,
) -> Result<String, String> {
    let guard = ImportGuard::capture(st, &plan, ImportKind::ItemSet)?;
    let lcu = st
        .lcu
        .lock()
        .unwrap()
        .clone()
        .ok_or("League client not connected")?;
    let catalog = st
        .catalog
        .lock()
        .unwrap()
        .clone()
        .ok_or("Data Dragon not loaded yet")?;
    guard.working(app, st, "itemset")?;
    let result = async {
        let key = catalog
            .champion_key(&plan.champion)
            .ok_or_else(|| format!("unknown champion {}", plan.champion))?;
        let known = *st.summoner_id.lock().unwrap(); // copy out: no guard across the await below
        let summoner_id = match known {
            Some(id) if id > 0 => id,
            _ => {
                let me = lcu.current_summoner().await.map_err(|e| e.to_string())?;
                me.get("summonerId")
                    .and_then(|v| v.as_u64())
                    .ok_or("no summonerId")?
            }
        };
        let set = itemset::build(&plan, st.pack_for(&plan.champion), &catalog, key);
        let current = lcu
            .item_sets(summoner_id)
            .await
            .map_err(|e| e.to_string())?;
        let payload = itemset::upsert(&current, set);
        guard.check(st)?;
        lcu.put_item_sets(summoner_id, &payload)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<String, String>(format!(
            "Item set '{}' is in the client",
            itemset::title(&plan.champion)
        ))
    }
    .await;
    guard.finish(app, st, "itemset", result)
}

pub async fn do_import_runes(app: &AppHandle, st: &Arc<App>) -> Result<String, String> {
    let plan = st
        .plan
        .lock()
        .unwrap()
        .clone()
        .ok_or("no build yet - wait for champ select")?;
    do_import_runes_for_plan(app, st, plan).await
}

pub async fn do_import_runes_for_plan(
    app: &AppHandle,
    st: &Arc<App>,
    plan: Plan,
) -> Result<String, String> {
    // Automatic retries and a manual click must not race to create two pages.
    static RUNE_WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let (guard, _write) = crate::rune_queue::acquire(&RUNE_WRITES, || {
        ImportGuard::capture(st, &plan, ImportKind::Runes)
    })
    .await?;
    let lcu = st
        .lcu
        .lock()
        .unwrap()
        .clone()
        .ok_or("League client not connected")?;
    let catalog = st
        .catalog
        .lock()
        .unwrap()
        .clone()
        .ok_or("Data Dragon not loaded yet")?;
    guard.working(app, st, "runes")?;
    let result = async {
        let ids = plan.runes.clone().ok_or("no rune page in this build")?;
        session::validate_rune_page(&ids, &catalog)?;
        let name = recall_core::brand::loadout_name(&plan.champion, plan.position.as_deref());
        let page = runes::page_value(&ids, &name);
        import_rune_page(&lcu, &guard, st, &page).await?;
        Ok::<String, String>(format!("Rune page '{name}' set ({})", plan.runes_summary))
    }
    .await;
    guard.finish(app, st, "runes", result)
}

pub async fn do_import_spells(app: &AppHandle, st: &Arc<App>) -> Result<String, String> {
    let plan = st
        .plan
        .lock()
        .unwrap()
        .clone()
        .ok_or("no build yet - wait for champ select")?;
    do_import_spells_for_plan(app, st, plan).await
}

pub async fn do_import_spells_for_plan(
    app: &AppHandle,
    st: &Arc<App>,
    plan: Plan,
) -> Result<String, String> {
    let guard = ImportGuard::capture(st, &plan, ImportKind::Spells)?;
    let lcu = st
        .lcu
        .lock()
        .unwrap()
        .clone()
        .ok_or("League client not connected")?;
    let current = st
        .lobby
        .lock()
        .unwrap()
        .as_ref()
        .map(|l| l.my_spells)
        .filter(|c| c.0 > 0 && c.1 > 0);
    guard.working(app, st, "spells")?;
    let result = async {
        let (a, b) = runes::order_spells(&plan.spell_ids, current)
            .ok_or("need exactly two summoner spells")?;
        if a == b || runes::spell_name(a).is_none() || runes::spell_name(b).is_none() {
            return Err("Recommended summoner spell IDs are invalid".into());
        }
        guard.check(st)?;
        let observed = st
            .lobby
            .lock()
            .unwrap()
            .as_ref()
            .map(|lobby| lobby.my_spells)
            .filter(|spells| spells.0 > 0 && spells.1 > 0);
        if observed != current && observed != Some((a, b)) {
            return Err("Your summoner spells changed; keeping your manual choice".into());
        }
        lcu.set_summoner_spells(a as u64, b as u64)
            .await
            .map_err(|e| e.to_string())?;
        let name = |id: u32| {
            runes::spell_name(id)
                .map(str::to_string)
                .unwrap_or_else(|| format!("spell {id}"))
        };
        Ok::<String, String>(format!("{} + {} selected", name(a), name(b)))
    }
    .await;
    guard.finish(app, st, "spells", result)
}

#[tauri::command]
pub fn get_state(st: State<'_, Arc<App>>) -> PanelState {
    st.snapshot()
}

#[tauri::command]
pub fn set_build_preference(
    app: AppHandle,
    st: State<'_, Arc<App>>,
    mode: BuildPreference,
) -> Result<(), String> {
    controller::change_preference(&app, &st, controller::PreferenceChange::Mode(mode))
}

#[tauri::command]
pub fn pin_item(app: AppHandle, st: State<'_, Arc<App>>, item_id: u32) -> Result<(), String> {
    controller::change_preference(&app, &st, controller::PreferenceChange::Pin(item_id))
}

#[tauri::command]
pub fn clear_item_pin(app: AppHandle, st: State<'_, Arc<App>>) -> Result<(), String> {
    controller::change_preference(&app, &st, controller::PreferenceChange::ClearPin)
}

#[tauri::command]
pub fn rate_decision(
    app: AppHandle,
    st: State<'_, Arc<App>>,
    decision_id: String,
    feedback: Feedback,
) -> Result<(), String> {
    controller::rate_decision(&app, &st, &decision_id, feedback)
}

#[tauri::command]
pub async fn import_item_set(app: AppHandle, st: State<'_, Arc<App>>) -> Result<String, String> {
    let st = st.inner().clone();
    do_import_item_set(&app, &st).await
}

#[tauri::command]
pub async fn import_runes(app: AppHandle, st: State<'_, Arc<App>>) -> Result<String, String> {
    let st = st.inner().clone();
    do_import_runes(&app, &st).await
}

#[tauri::command]
pub async fn import_spells(app: AppHandle, st: State<'_, Arc<App>>) -> Result<String, String> {
    let st = st.inner().clone();
    do_import_spells(&app, &st).await
}

#[tauri::command]
pub fn set_collapsed(
    app: AppHandle,
    st: State<'_, Arc<App>>,
    collapsed: bool,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let height = if collapsed {
            crate::PANEL_H_COLLAPSED
        } else {
            crate::PANEL_H
        };
        window
            .set_size(tauri::LogicalSize::new(crate::PANEL_W, height))
            .map_err(|e| e.to_string())?;
    }
    // Not persisted on purpose: the panel always starts expanded (see settings.rs).
    st.update(&app, |p| p.collapsed = collapsed);
    Ok(())
}

#[tauri::command]
pub fn quit(app: AppHandle, st: tauri::State<'_, Arc<App>>) {
    // Under --autostart the x button means "not this game": hide until the client restarts.
    if st.autostart {
        *st.dismissed.lock().unwrap() = true;
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
        log::info!("autostart: panel dismissed until the client restarts");
        return;
    }
    app.exit(0);
}
