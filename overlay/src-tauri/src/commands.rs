//! Commands the webview can invoke.
use crate::App;
use featherstorm_core::itemset;
use featherstorm_core::runes;
use featherstorm_core::state::PanelState;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

fn set_import(app: &AppHandle, st: &App, which: &str, status: &str) {
    let status = status.to_string();
    let which = which.to_string();
    st.update(app, |p| match which.as_str() {
        "itemset" => p.imports.itemset = status,
        "runes" => p.imports.runes = status,
        _ => p.imports.spells = status,
    });
}

#[tauri::command]
pub fn get_state(st: State<'_, Arc<App>>) -> PanelState {
    st.snapshot()
}

#[tauri::command]
pub async fn import_item_set(app: AppHandle, st: State<'_, Arc<App>>) -> Result<String, String> {
    let st = st.inner().clone();
    let lcu = st.lcu.lock().unwrap().clone().ok_or("League client not connected")?;
    let plan = st.plan.lock().unwrap().clone().ok_or("no build yet - wait for champ select")?;
    let catalog = st.catalog.lock().unwrap().clone().ok_or("Data Dragon not loaded yet")?;
    set_import(&app, &st, "itemset", "working");
    let result = async {
        let key = catalog.champion_key(&st.pack.champion).ok_or_else(|| format!("unknown champion {}", st.pack.champion))?;
        let summoner_id = match *st.summoner_id.lock().unwrap() {
            Some(id) => id,
            None => 0,
        };
        let summoner_id = if summoner_id > 0 {
            summoner_id
        } else {
            let me = lcu.current_summoner().await.map_err(|e| e.to_string())?;
            me.get("summonerId").and_then(|v| v.as_u64()).ok_or("no summonerId")?
        };
        let set = itemset::build(&plan, &st.pack, &catalog, key);
        let current = lcu.item_sets(summoner_id).await.map_err(|e| e.to_string())?;
        let payload = itemset::upsert(&current, set);
        lcu.put_item_sets(summoner_id, &payload).await.map_err(|e| e.to_string())?;
        Ok::<String, String>(format!("Item set '{}' is in the client", itemset::title(&st.pack)))
    }
    .await;
    match &result {
        Ok(_) => set_import(&app, &st, "itemset", "done"),
        Err(e) => set_import(&app, &st, "itemset", &format!("error: {e}")),
    }
    result
}

#[tauri::command]
pub async fn import_runes(app: AppHandle, st: State<'_, Arc<App>>) -> Result<String, String> {
    let st = st.inner().clone();
    let lcu = st.lcu.lock().unwrap().clone().ok_or("League client not connected")?;
    let catalog = st.catalog.lock().unwrap().clone().ok_or("Data Dragon not loaded yet")?;
    set_import(&app, &st, "runes", "working");
    let result = async {
        let page = runes::build_page(&st.pack.runes, &catalog, &st.pack.runes.name).map_err(|e| e.to_string())?;
        let name = runes::import(&lcu, page).await.map_err(|e| e.to_string())?;
        Ok::<String, String>(format!("Rune page '{name}' set"))
    }
    .await;
    match &result {
        Ok(_) => set_import(&app, &st, "runes", "done"),
        Err(e) => set_import(&app, &st, "runes", &format!("error: {e}")),
    }
    result
}

#[tauri::command]
pub async fn import_spells(app: AppHandle, st: State<'_, Arc<App>>) -> Result<String, String> {
    let st = st.inner().clone();
    let lcu = st.lcu.lock().unwrap().clone().ok_or("League client not connected")?;
    let spells = st
        .plan
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.spells.clone())
        .unwrap_or_else(|| st.pack.spells.clone());
    set_import(&app, &st, "spells", "working");
    let result = async {
        if spells.len() != 2 {
            return Err("need exactly two summoner spells".to_string());
        }
        let a = runes::spell_id(&spells[0]).ok_or(format!("unknown spell {}", spells[0]))?;
        let b = runes::spell_id(&spells[1]).ok_or(format!("unknown spell {}", spells[1]))?;
        lcu.set_summoner_spells(a, b).await.map_err(|e| e.to_string())?;
        Ok::<String, String>(format!("{} + {} selected", spells[0], spells[1]))
    }
    .await;
    match &result {
        Ok(_) => set_import(&app, &st, "spells", "done"),
        Err(e) => set_import(&app, &st, "spells", &format!("error: {e}")),
    }
    result
}

#[tauri::command]
pub fn set_collapsed(app: AppHandle, st: State<'_, Arc<App>>, collapsed: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let height = if collapsed { crate::PANEL_H_COLLAPSED } else { crate::PANEL_H };
        window
            .set_size(tauri::LogicalSize::new(crate::PANEL_W, height))
            .map_err(|e| e.to_string())?;
    }
    {
        let mut s = st.settings.lock().unwrap();
        s.collapsed = collapsed;
        let _ = crate::settings::save(&s);
    }
    st.update(&app, |p| p.collapsed = collapsed);
    Ok(())
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
