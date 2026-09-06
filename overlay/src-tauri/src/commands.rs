//! Imports into the client (runes, summoner spells, item set) and the commands the webview invokes.
//! The poller calls the `do_*` functions for auto-import; the buttons call the same code.
use crate::App;
use featherstorm_core::itemset;
use featherstorm_core::runes;
use featherstorm_core::state::PanelState;
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

pub async fn do_import_item_set(app: &AppHandle, st: &Arc<App>) -> Result<String, String> {
    let lcu = st.lcu.lock().unwrap().clone().ok_or("League client not connected")?;
    let plan = st.plan.lock().unwrap().clone().ok_or("no build yet - wait for champ select")?;
    let catalog = st.catalog.lock().unwrap().clone().ok_or("Data Dragon not loaded yet")?;
    set_import(app, st, "itemset", "working");
    let result = async {
        let key = catalog.champion_key(&plan.champion).ok_or_else(|| format!("unknown champion {}", plan.champion))?;
        let known = *st.summoner_id.lock().unwrap(); // copy out: no guard across the await below
        let summoner_id = match known {
            Some(id) if id > 0 => id,
            _ => {
                let me = lcu.current_summoner().await.map_err(|e| e.to_string())?;
                me.get("summonerId").and_then(|v| v.as_u64()).ok_or("no summonerId")?
            }
        };
        let set = itemset::build(&plan, st.pack_for(&plan.champion), &catalog, key);
        let current = lcu.item_sets(summoner_id).await.map_err(|e| e.to_string())?;
        let payload = itemset::upsert(&current, set);
        lcu.put_item_sets(summoner_id, &payload).await.map_err(|e| e.to_string())?;
        Ok::<String, String>(format!("Item set '{}' is in the client", itemset::title(&plan.champion)))
    }
    .await;
    finish(app, st, "itemset", &result);
    result
}

pub async fn do_import_runes(app: &AppHandle, st: &Arc<App>) -> Result<String, String> {
    let lcu = st.lcu.lock().unwrap().clone().ok_or("League client not connected")?;
    let plan = st.plan.lock().unwrap().clone().ok_or("no build yet - wait for champ select")?;
    set_import(app, st, "runes", "working");
    let result = async {
        let ids = plan.runes.clone().ok_or("no rune page in this build")?;
        let name = match &plan.position {
            Some(pos) => format!("Featherstorm {} {pos}", plan.champion),
            None => format!("Featherstorm {}", plan.champion),
        };
        let page = runes::page_value(&ids, &name);
        let name = runes::import(&lcu, page).await.map_err(|e| e.to_string())?;
        Ok::<String, String>(format!("Rune page '{name}' set ({})", plan.runes_summary))
    }
    .await;
    finish(app, st, "runes", &result);
    result
}

pub async fn do_import_spells(app: &AppHandle, st: &Arc<App>) -> Result<String, String> {
    let lcu = st.lcu.lock().unwrap().clone().ok_or("League client not connected")?;
    let plan = st.plan.lock().unwrap().clone().ok_or("no build yet - wait for champ select")?;
    let current = st.lobby.lock().unwrap().as_ref().map(|l| l.my_spells).filter(|c| c.0 > 0 && c.1 > 0);
    set_import(app, st, "spells", "working");
    let result = async {
        let (a, b) = runes::order_spells(&plan.spell_ids, current).ok_or("need exactly two summoner spells")?;
        lcu.set_summoner_spells(a as u64, b as u64).await.map_err(|e| e.to_string())?;
        let name = |id: u32| runes::spell_name(id).map(str::to_string).unwrap_or_else(|| format!("spell {id}"));
        Ok::<String, String>(format!("{} + {} selected", name(a), name(b)))
    }
    .await;
    finish(app, st, "spells", &result);
    result
}

#[tauri::command]
pub fn get_state(st: State<'_, Arc<App>>) -> PanelState {
    st.snapshot()
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
pub fn set_collapsed(app: AppHandle, st: State<'_, Arc<App>>, collapsed: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let height = if collapsed { crate::PANEL_H_COLLAPSED } else { crate::PANEL_H };
        window
            .set_size(tauri::LogicalSize::new(crate::PANEL_W, height))
            .map_err(|e| e.to_string())?;
    }
    // Not persisted on purpose: the panel always starts expanded (see settings.rs).
    st.update(&app, |p| p.collapsed = collapsed);
    Ok(())
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
